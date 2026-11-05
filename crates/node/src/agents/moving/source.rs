//! The machine a session moves away from: waits for its turn, or stops it,
//! then ends the session. The transcript stays, and `beep-sessions/1`
//! keeps serving it.

use super::{
    Env,
    saved::{Phase, Saved},
};
use anyhow::{Context, Result};
use beep_proto::node::agent::Status;
use beep_proto::node::mesh_state::SessionMove;

use crate::agents::session::Command;

pub(super) async fn run(env: &Env, record: &SessionMove) -> Result<()> {
    let done = |saved: &Saved| saved.at == record.at && saved.phase != Phase::Working;
    if env.state.load(&record.id).is_some_and(|saved| done(&saved)) {
        return Ok(());
    }
    env.agents.until_reopened().await;
    if let Some(session) = env
        .agents
        .get(&record.id)
        .filter(|session| session.live.borrow().status() != Status::Ended)
    {
        session
            .order(Command::Moving { stop: record.stop })
            .await
            .map_err(anyhow::Error::msg)?;
        // Idle is the end of the turn; a session that ends by itself ends
        // the wait too.
        let mut live = session.live.clone();
        live.wait_for(|live| matches!(live.status(), Status::Idle | Status::Ended))
            .await
            .context("the session vanished")?;
        session.close().await;
    }
    env.state
        .save(
            &record.id,
            &Saved {
                at: record.at,
                phase: Phase::Done,
                reason: None,
            },
        )
        .context("keeping that the session ended")
}

/// The move is cancelled: a session still waiting for its turn goes on as
/// it was, queue and all.
pub(super) async fn cancel(env: &Env, id: &str) {
    let Some(session) = env
        .agents
        .get(id)
        .filter(|session| session.live.borrow().status() != Status::Ended)
    else {
        return;
    };
    if let Err(e) = session.order(Command::NotMoving).await {
        log::debug!("session {id} was not told the move is off: {e}");
    }
}
