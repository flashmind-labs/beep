//! The machine a session moves away from: lets the turn and the messages
//! queued before the move finish, or stops them, waits until the target
//! can take the session, then ends it and has its files scanned. The
//! transcript stays, and `beep-sessions/1` keeps serving it. If the target
//! gives up, the session starts again here.

use super::{
    Env,
    progress::Target,
    saved::{Phase, Saved},
};
use crate::agents::{OpenSession, Session, session::Command};
use anyhow::{Context, Result, anyhow};
use beep_proto::node::{agent::Status, mesh_state::SessionMove};
use std::sync::Arc;
use tokio::sync::watch;

pub(super) async fn run(
    env: &Env,
    record: &SessionMove,
    target: &mut watch::Receiver<Target>,
) -> Result<()> {
    let saved = env.state.load(&record.id).filter(|s| s.at == record.at);
    if saved.as_ref().is_some_and(|saved| saved.is_over(record.at)) {
        return Ok(());
    }
    env.agents.until_reopened().await;
    // A target reads the session as running until its files are scanned.
    let held = env.agents.moves.hold(&record.id);
    // Set once the session ended here: a restart finds it in what is kept.
    let mut open = saved.and_then(|saved| saved.open);
    if open.is_none()
        && let Some(session) = live(env, &record.id)
    {
        match end(env, record, &session, target).await {
            Ok(done) => open = done,
            Err(e) => return fail(env, record, None, &e).await,
        }
    }
    if let Err(e) = scan(env, record).await {
        return fail(env, record, open, &e).await;
    }
    drop(held);
    land(env, record, open, target).await
}

fn live(env: &Env, id: &str) -> Option<Arc<Session>> {
    env.agents
        .get(id)
        .filter(|session| session.live.borrow().status() != Status::Ended)
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
    live.wait_for(|live| match live.status() {
        Status::Ended => true,
        Status::Idle => live.info.state.queued == 0,
        _ => false,
    })
    .await
    .context("the session vanished")?;
    if live.borrow().status() == Status::Ended {
        return Ok(None);
    }
    // The session ends only once the target found it can take it, so a
    // machine that cannot never costs the session.
    loop {
        let now = target.borrow_and_update().clone();
        match now {
            Target::Failed(reason) => return Err(anyhow!(reason)),
            Target::Taken | Target::Landed => break,
            Target::Unknown => {}
        }
        tokio::select! {
            changed = target.changed() => changed.context("the move lost its target")?,
            ended = live.wait_for(|live| live.status() == Status::Ended) => {
                ended.context("the session vanished")?;
                return Ok(None);
            }
        }
    }
    let open = env.agents.open_session(&record.id);
    session.close().await;
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
    Ok(open)
}

/// Has the project scanned now, so what the session wrote last is in the
/// list the target pulls. A folder that is not scanned would let it resume
/// on files from before.
async fn scan(env: &Env, record: &SessionMove) -> Result<()> {
    let Some(syncs) = &env.syncs else {
        return Ok(());
    };
    syncs
        .rescan(&record.project)
        .await
        .map(|_| ())
        .with_context(|| format!("scanning {} before it moves", record.project))
}

/// Waits until the target runs the session, or gives up.
async fn land(
    env: &Env,
    record: &SessionMove,
    open: Option<OpenSession>,
    target: &mut watch::Receiver<Target>,
) -> Result<()> {
    loop {
        let now = target.borrow_and_update().clone();
        match now {
            Target::Landed => {
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
/// it was.
pub(super) async fn cancel(env: &Env, id: &str) {
    let Some(session) = live(env, id) else {
        return;
    };
    if let Err(e) = session.order(Command::NotMoving).await {
        log::debug!("session {id} was not told the move is off: {e}");
    }
}
