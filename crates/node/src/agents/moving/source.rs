//! The machine a session moves away from: lets the turn and the messages
//! queued before the move finish, or stops them, waits until the target
//! can take the session, then ends it. The transcript stays, and
//! `beep-sessions/1` keeps serving it and the session's worktrees. Once the
//! session runs on the target, the worktrees it took are let go here. If
//! the target gives up, the session starts again here.

use super::{
    Env,
    dial::outcome,
    progress::Target,
    saved::{MOVED_AWAY, Outgoing, Phase, Previous, Saved, SentTree},
    worktree,
};
use crate::agents::live::Live;
use crate::agents::{OpenSession, Session, session::Command};
use anyhow::{Context, Result, anyhow};
use beep_proto::SessionId;
use beep_proto::node::{agent::Status, mesh_state::SessionMove};
use beep_proto::watched;
use iroh::EndpointId;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    sync::watch,
    task::spawn_blocking,
    time::{sleep, timeout},
};

/// How long a cancelled move waits for the session to end, and for the
/// target to say whether it took the session anyway.
const CLOSE_WITHIN: Duration = Duration::from_secs(30);
const ASK_WITHIN: Duration = Duration::from_secs(20);
const ASK_AGAIN: Duration = Duration::from_millis(500);

pub(super) async fn run(
    env: &Env,
    record: &SessionMove,
    target: &mut watch::Receiver<Target>,
) -> Result<()> {
    let mut saved = env.state.load(&record.id);

    if saved.as_ref().is_some_and(|saved| saved.is_over(record.at)) {
        return Ok(());
    }

    if !env.agents.reopen.reopened() {
        log::info!(
            "session {}: waiting for the sessions open before the restart to resume",
            record.id
        );
    }

    env.agents.reopen.until_reopened().await;
    let next = Outgoing {
        at: record.at,
        to: record.to,
    };
    if let Some(old) = saved.as_ref() {
        let before = old.before(next);
        if before == Previous::Settling
            && let Some(to) = old.destination
            && matches!(
                cancel(env, &record.id, old.at, to).await,
                Recovery::Released
            )
        {
            env.state.clear(&record.id, old.at);
            saved = None;
        } else if before != Previous::Released {
            env.agents.moves.refuse(&record.id, MOVED_AWAY);
            return Err(anyhow!(MOVED_AWAY));
        }
    }

    // A target reads the session as running until it ended here.
    let held = env.agents.moves.hold(&record.id);
    // Set once the session ended here: a restart finds it in what is kept.
    let mut open = saved
        .filter(|saved| saved.at == record.at)
        .and_then(|saved| saved.open);

    // A session still live ends now, even if a restart kept how to start
    // it again: that is saved before it is closed.
    if let Some(session) = live(env, &record.id) {
        match end(env, record, &session, target).await {
            Ok(done) => open = done,
            Err(e) => return fail(env, record, None, &e).await,
        }
    }

    drop(held);
    land(env, record, open, target).await
}

fn live(env: &Env, id: &SessionId) -> Option<Arc<Session>> {
    env.agents
        .sessions
        .get(id)
        .filter(|session| watched::read(&session.live, Live::status) != Status::Ended)
}

/// Waits for the turn and what was queued before the move to be done, and
/// for the target to take the session; ends it. What starts it again.
async fn end(
    env: &Env,
    record: &SessionMove,
    session: &Session,
    target: &mut watch::Receiver<Target>,
) -> Result<Option<OpenSession>> {
    session
        .order(Command::Moving { stop: record.stop })
        .await
        .map_err(anyhow::Error::msg)?;
    // Idle with nothing queued is the end; a session that ends by itself
    // ends the wait too.
    let mut live = session.live.clone();
    watched::wait_for(&mut live, |live| match live.status() {
        Status::Ended => true,
        Status::Idle => live.info.state.queued == 0 && live.info.state.subagents == 0,
        _ => false,
    })
    .await
    .context("the session vanished")?;

    if watched::read(&live, Live::status) == Status::Ended {
        return Ok(None);
    }
    // The session ends only once the target found it can take it, so a
    // machine that cannot never costs the session.
    loop {
        let now = watched::read_and_update(target, Clone::clone);

        match now {
            Target::Failed(reason) => return Err(anyhow!(reason)),
            Target::Taken | Target::Starting | Target::Landed => break,
            Target::Unknown => {}
        }

        tokio::select! {
            changed = target.changed() => changed.context("the move lost its target")?,
            ended = watched::wait_for(&mut live, |live| live.status() == Status::Ended) => {
                ended.context("the session vanished")?;
                return Ok(None);
            }
        }
    }

    // Kept before the session closes, so a move taken back at any point
    // after finds how to start it again.
    let open = env.agents.reopen.open_session(&record.id);
    // Worktrees sent before a restart stay noted: the target does not ask
    // for them again.
    let sent = env
        .state
        .load(&record.id)
        .filter(|saved| saved.at == record.at)
        .map(|saved| saved.sent)
        .unwrap_or_default();
    env.state
        .save(
            &record.id,
            &Saved {
                at: record.at,
                phase: Phase::Closed,
                reason: None,
                open: open.clone(),
                placed: Vec::new(),
                sent,
                destination: Some(record.to),
            },
        )
        .context("keeping that the session ended")?;
    session.close().await;
    Ok(open)
}

/// Waits until the target runs the session, or gives up.
async fn land(
    env: &Env,
    record: &SessionMove,
    open: Option<OpenSession>,
    target: &mut watch::Receiver<Target>,
) -> Result<()> {
    loop {
        let now = watched::read_and_update(target, Clone::clone);

        match now {
            Target::Landed => {
                return landed(env, &record.id, record.at, record.to).await;
            }
            Target::Failed(reason) => return fail(env, record, open, &anyhow!(reason)).await,
            // Starting there can still fail: the worktrees stay until it runs.
            Target::Taken | Target::Starting | Target::Unknown => {}
        }

        target.changed().await.context("the move lost its target")?;
    }
}

/// Lets go of the worktrees sent for the session, which runs elsewhere
/// now: each is detached so its branch is the target's alone, and what was
/// sent is taken out of it unless it changed since.
async fn landed(env: &Env, id: &SessionId, at: u64, to: EndpointId) -> Result<()> {
    let sent = env
        .state
        .load(id)
        .filter(|saved| saved.at == at)
        .map(|saved| saved.sent)
        .unwrap_or_default();

    for SentTree { dir, held } in sent {
        let shown = dir.display().to_string();
        let done = spawn_blocking(move || worktree::detach_clean(&dir, Some(&held))).await;

        match done {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => log::warn!("the worktree {shown} stays on its branch: {e:#}"),
            Err(e) => log::warn!("letting go of the worktree {shown} failed: {e}"),
        }
    }

    let mut saved = Saved::over(at, Phase::Done, None);
    saved.destination = Some(to);
    env.state
        .save(id, &saved)
        .context("keeping that the session moved")
}

/// The move is off: the session goes on here, as it was, and a target that
/// asks hears why. Starts it again when it already ended.
async fn fail(
    env: &Env,
    record: &SessionMove,
    open: Option<OpenSession>,
    why: &anyhow::Error,
) -> Result<()> {
    let reason = format!("{why:#}");
    env.agents.moves.refuse(&record.id, &reason);

    match (live(env, &record.id), open) {
        (Some(session), _) => {
            if let Err(e) = session.order(Command::NotMoving).await {
                log::debug!("session {} was not told the move is off: {e}", record.id);
            }
        }
        (None, Some(open)) => {
            if let Err(e) = env.agents.reopen_one(open).await {
                log::warn!(
                    "session {} could not start again after its move failed: {e:#}",
                    record.id
                );
            }
        }
        (None, None) => {}
    }

    env.state
        .save(
            &record.id,
            &Saved::over(record.at, Phase::Failed, Some(reason)),
        )
        .context("keeping that the move failed")?;
    Err(anyhow!("the move was given up: {why:#}"))
}

/// The move is cancelled: a session still waiting for its turn goes on as
/// it was, and one this side ended starts again, unless the target already
/// starts or runs it (`to` says, about the move made at `at`).
pub(super) enum Recovery {
    Released,
    TargetMayRun,
}

pub(super) async fn cancel(env: &Env, id: &SessionId, at: u64, to: EndpointId) -> Recovery {
    let closed = env
        .state
        .load(id)
        .filter(|saved| saved.at == at && saved.phase == Phase::Closed);

    if let Some(session) = live(env, id) {
        if closed.is_none() {
            if let Err(e) = session.order(Command::NotMoving).await {
                log::debug!("session {id} was not told the move is off: {e}");
            }

            return Recovery::Released;
        }
        // Cancellation may have interrupted the original close before the
        // command was sent. Closing a session already on its way out is safe.
        if timeout(CLOSE_WITHIN, session.close()).await.is_err() {
            log::warn!("session {id} did not end in time, so it is not started again");
            return Recovery::TargetMayRun;
        }
    }

    let Some(closed) = closed else {
        return Recovery::Released;
    };

    match target_outcome(env, id, at, to).await {
        Some(Target::Landed) => {
            if let Err(e) = landed(env, id, at, to).await {
                log::warn!("session {id}: {e:#}");
            }
            return Recovery::TargetMayRun;
        }
        Some(Target::Failed(_)) => {}
        _ => {
            log::info!("session {id} is not started again here: the target may run it");
            return Recovery::TargetMayRun;
        }
    }

    if let Some(open) = closed.open
        && let Err(e) = env.agents.reopen_one(open).await
    {
        log::warn!("session {id} could not start again after its move was cancelled: {e:#}");
    }
    Recovery::Released
}

/// Whether the old target landed or rolled back. Silence keeps the source closed.
async fn target_outcome(env: &Env, id: &SessionId, at: u64, to: EndpointId) -> Option<Target> {
    timeout(ASK_WITHIN, async {
        loop {
            match outcome(env, to, id, at).await {
                Ok(ended) => return ended,
                Err(_) => sleep(ASK_AGAIN).await,
            }
        }
    })
    .await
    .ok()
}
