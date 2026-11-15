//! The machine a session moves away from: lets the turn and the messages
//! queued before the move finish, or stops them, waits until the target
//! can take the session, then ends it. The transcript stays, and
//! `beep-sessions/1` keeps serving it and the session's worktrees. Once the
//! session runs on the target, the worktrees it took are let go here. If
//! the target gives up, the session starts again here.

use super::{
    Env,
    progress::Target,
    saved::{Phase, Saved},
    worktree,
};
use crate::agents::live::Live;
use crate::agents::{OpenSession, Session, session::Command};
use anyhow::{Context, Result, anyhow};
use beep_proto::node::{agent::Status, mesh_state::SessionMove};
use beep_proto::watched;
use std::sync::Arc;
use tokio::{sync::watch, task::spawn_blocking};

pub(super) async fn run(
    env: &Env,
    record: &SessionMove,
    target: &mut watch::Receiver<Target>,
) -> Result<()> {
    let saved = env.state.load(&record.id).filter(|s| s.at == record.at);

    if saved.as_ref().is_some_and(|saved| saved.is_over(record.at)) {
        return Ok(());
    }

    if !env.agents.reopened() {
        log::info!(
            "session {}: waiting for the sessions open before the restart to resume",
            record.id
        );
    }

    env.agents.until_reopened().await;
    // A target reads the session as running until it ended here.
    let held = env.agents.moves.hold(&record.id);
    // Set once the session ended here: a restart finds it in what is kept.
    let mut open = saved.and_then(|saved| saved.open);

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

fn live(env: &Env, id: &str) -> Option<Arc<Session>> {
    env.agents
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
            Target::Taken | Target::Landed => break,
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
    let open = env.agents.open_session(&record.id);
    env.state
        .save(
            &record.id,
            &Saved {
                at: record.at,
                phase: Phase::Closed,
                reason: None,
                open: open.clone(),
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
                let_go(env, &record.id).await;
                return env
                    .state
                    .save(&record.id, &Saved::over(record.at, Phase::Done, None))
                    .context("keeping that the session moved");
            }
            Target::Failed(reason) => return fail(env, record, open, &anyhow!(reason)).await,
            Target::Taken | Target::Unknown => {}
        }

        target.changed().await.context("the move lost its target")?;
    }
}

/// Lets go of the worktrees sent for session `id`, which runs elsewhere
/// now: each is detached so its branch is the target's alone, and what was
/// sent is taken out of it unless it changed since.
async fn let_go(env: &Env, id: &str) {
    for (dir, sent) in env.agents.moves.take_sent(id) {
        let shown = dir.display().to_string();
        let done = spawn_blocking(move || worktree::detach_clean(&dir, Some(&sent))).await;

        match done {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => log::warn!("the worktree {shown} stays on its branch: {e:#}"),
            Err(e) => log::warn!("letting go of the worktree {shown} failed: {e}"),
        }
    }
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
/// it was, and one this side already ended starts again.
pub(super) async fn cancel(env: &Env, id: &str) {
    if let Some(session) = live(env, id) {
        if let Err(e) = session.order(Command::NotMoving).await {
            log::debug!("session {id} was not told the move is off: {e}");
        }

        return;
    }

    let Some(open) = env.state.load(id).and_then(|saved| saved.open) else {
        return;
    };

    if let Err(e) = env.agents.reopen_one(open).await {
        log::warn!("session {id} could not start again after its move was cancelled: {e:#}");
    }
}
