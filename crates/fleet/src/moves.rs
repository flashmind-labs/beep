//! Moving a coding agent session to another machine: the desktop writes the
//! mesh's `session-move/<id>` record, which both machines act on, and asks
//! the target how it goes.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{WRITES, read, scan},
};
use beep_mesh::{
    Key,
    machine::sessions::{MoveState, move_progress},
};
use beep_proto::{
    AgentSlug,
    node::mesh_state::{SessionMove, StateValue, SyncSpec},
};
use iroh::EndpointId;
use std::{
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

/// A session to move, as the desktop knows it from the source's list.
pub struct MoveAsk<'a> {
    pub agent: &'a str,
    /// The agent's own id.
    pub id: &'a str,
    pub from: EndpointId,
    pub to: EndpointId,
    pub project: &'a str,
    /// The folder the session works in on the source.
    pub cwd: &'a str,
    /// It works in a linked worktree of the project, not its checkout.
    pub linked: bool,
    pub mode: Option<String>,
    /// Stop the turn instead of waiting for it.
    pub stop: bool,
}

impl Desk {
    /// Asks `ask.to` to take the session over from `ask.from`. Refused, in
    /// words for the user, when the session is one that cannot move yet.
    pub async fn move_session(&self, ask: MoveAsk<'_>) -> Result<SessionMove> {
        if !matches!(ask.agent, "claude" | "codex" | "pi") {
            return Err(refused(format!(
                "{} sessions cannot move between machines yet",
                ask.agent
            )));
        }
        if ask.linked {
            return Err(refused(
                "a session in a worktree cannot move between machines yet".to_owned(),
            ));
        }
        if ask.from == ask.to {
            return Err(refused("the session is on that machine already".to_owned()));
        }
        let spec: SyncSpec = read(&self.mesh, &Key::Sync(ask.project.to_owned()))
            .ok_or_else(|| refused(format!("{} is not synced between machines", ask.project)))?;
        let theirs = spec
            .folders
            .get(&ask.from)
            .ok_or_else(|| refused("the source has no checkout of the project".to_owned()))?;
        if !spec.folders.contains_key(&ask.to) {
            return Err(refused(
                "that machine has no checkout of the project: import it first".to_owned(),
            ));
        }
        let path = Path::new(ask.cwd)
            .strip_prefix(theirs)
            .ok()
            .filter(|rest| {
                rest.components()
                    .all(|part| matches!(part, Component::Normal(_)))
            })
            .and_then(Path::to_str)
            .ok_or_else(|| refused("the session works outside the project's checkout".to_owned()))?
            .to_owned();
        let record = SessionMove {
            agent: AgentSlug::from(ask.agent),
            id: ask.id.to_owned(),
            from: ask.from,
            to: ask.to,
            project: ask.project.to_owned(),
            path,
            mode: ask.mode,
            stop: ask.stop,
            at: now_ms(),
        };
        let _one = WRITES.lock().await;
        self.forget_old_moves(record.at).await?;
        self.mesh
            .set(
                Key::SessionMove(ask.id.to_owned()),
                Some(record.to_bytes().map_err(anyhow::Error::from)?),
            )
            .await?;
        Ok(record)
    }

    /// Deletes the records of moves that are over by now, so the mesh does
    /// not keep one for every session that ever moved. Done under the write
    /// lock, as the new record is written.
    async fn forget_old_moves(&self, now: u64) -> Result<()> {
        for (key, record) in scan::<SessionMove>(&self.mesh, "session-move/") {
            if now.saturating_sub(record.at) > KEEP_MOVES_MS {
                self.mesh.set(key, None).await?;
            }
        }
        Ok(())
    }

    /// Whether the node of `machine` takes part in moves: a node from before
    /// them does not, and a session must not be sent to it.
    pub async fn takes_moves(&self, machine: EndpointId) -> Result<bool> {
        let client = self.peers.client(machine).await?;
        Ok(!matches!(
            move_progress(&client, PROBE).await?,
            MoveState::NeedsUpdate
        ))
    }

    /// Takes back the move of session `id`: the machines stop it unless
    /// the target already resumed the session.
    pub async fn cancel_move(&self, id: &str) -> Result<()> {
        let _one = WRITES.lock().await;
        self.mesh.set(Key::SessionMove(id.to_owned()), None).await?;
        Ok(())
    }

    /// The moves in the mesh, the latest record of each session.
    pub fn moves(&self) -> Vec<SessionMove> {
        scan::<SessionMove>(&self.mesh, "session-move/")
            .into_iter()
            .map(|(_, record)| record)
            .collect()
    }

    /// How the move of `id` stands, as its target says.
    pub async fn move_state(&self, id: &str) -> Result<MoveState> {
        let record: SessionMove =
            read(&self.mesh, &Key::SessionMove(id.to_owned())).ok_or(FleetError::NotFound)?;
        let client = self.peers.client(record.to).await?;
        Ok(move_progress(&client, id).await?)
    }
}

/// How long the record of a move is kept: a session that moved shows once
/// while it is, and a move still waiting for a machine that is away is not
/// lost in a few days.
const KEEP_MOVES_MS: u64 = 30 * 24 * 3600 * 1000;

/// A session id no move has, to ask a node whether it knows moves at all.
const PROBE: &str = "probe";

fn refused(message: String) -> FleetError {
    FleetError::Refused(message)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}
