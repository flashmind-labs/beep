//! Moving a coding agent session to another machine: the desktop writes the
//! mesh's `session-move/<id>` record, which both machines act on, and asks
//! the target how it goes.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{Place, WRITES, now_ms, place_key, read, scan},
};
use beep_mesh::{
    Key,
    machine::sessions::{MoveState, move_progress},
};
use beep_proto::SessionId;
use beep_proto::{
    AgentSlug, ModelId,
    node::mesh_state::{SessionMove, StateValue, SyncSpec},
};
use iroh::EndpointId;
use std::path::{Component, Path};

/// A session to move, as the desktop knows it from the source's list.
pub struct MoveAsk<'a> {
    pub agent: &'a str,
    /// The agent's own id.
    pub id: &'a SessionId,
    pub from: EndpointId,
    pub to: EndpointId,
    pub project: &'a str,
    /// The folder the session works in on the source: its project folder
    /// or a folder below it.
    pub cwd: &'a str,
    pub mode: Option<String>,
    /// The model it runs on.
    pub model: Option<ModelId>,
    /// The linked worktrees it created on the source.
    pub worktrees: Vec<String>,
    /// Stop the turn instead of waiting for it.
    pub stop: bool,
}

impl Desk {
    /// Asks `ask.to` to take the session over from `ask.from`. Refused, in
    /// words for the user, when the session is one that cannot move yet.
    pub async fn move_session(&self, ask: MoveAsk<'_>) -> Result<SessionMove> {
        if !matches!(ask.agent, "claude" | "codex" | "pi") {
            return Err(FleetError::refused(format!(
                "{} sessions cannot move between machines yet",
                ask.agent
            )));
        }

        if ask.from == ask.to {
            return Err(FleetError::refused(
                "the session is on that machine already",
            ));
        }

        let theirs = self
            .project_folder(ask.project, ask.from)
            .ok_or_else(|| FleetError::refused("the source has no checkout of the project"))?;

        if self.project_folder(ask.project, ask.to).is_none() {
            return Err(FleetError::refused(
                "that machine has no checkout of the project: import it first",
            ));
        }

        // The session resumes in the target's project folder, or the same
        // folder below it.
        let path = inside(ask.cwd, &theirs)
            .ok_or_else(|| FleetError::refused("the session works outside the project's checkout"))?
            .to_owned();
        let record = SessionMove {
            agent: AgentSlug::from(ask.agent),
            id: ask.id.clone(),
            from: ask.from,
            to: ask.to,
            project: ask.project.to_owned(),
            path,
            mode: ask.mode,
            stop: ask.stop,
            at: now_ms(),
            // Sessions run in the project folder, never in a worktree.
            worktree: None,
            model: ask.model,
            worktrees: ask.worktrees,
        };
        let _one = WRITES.lock().await;
        self.forget_old_moves(record.at).await?;
        self.mesh
            .set(
                Key::SessionMove(ask.id.clone()),
                Some(record.to_bytes().map_err(anyhow::Error::from)?),
            )
            .await?;
        Ok(record)
    }

    /// The folder `project` has on `machine`: from its `sync/<name>` record
    /// when that names the machine, else from the machine's place record. A
    /// project imported on one machine at a time has only the place records.
    fn project_folder(&self, project: &str, machine: EndpointId) -> Option<String> {
        read::<SyncSpec>(&self.mesh, &Key::Sync(project.to_owned()))
            .and_then(|mut spec| spec.folders.remove(&machine))
            .or_else(|| {
                read::<Place>(&self.mesh, &place_key(project, machine)).map(|place| place.folder)
            })
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
            move_progress(&client, &SessionId::from(PROBE)).await?,
            MoveState::NeedsUpdate
        ))
    }

    /// Takes back the move of session `id`: the machines stop it unless
    /// the target already resumed the session.
    pub async fn cancel_move(&self, id: &SessionId) -> Result<()> {
        let _one = WRITES.lock().await;
        self.mesh.set(Key::SessionMove(id.clone()), None).await?;
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
    pub async fn move_state(&self, id: &SessionId) -> Result<MoveState> {
        let record: SessionMove =
            read(&self.mesh, &Key::SessionMove(id.clone())).ok_or(FleetError::NotFound)?;
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

/// `cwd` relative to `base`, when it is `base` or a folder below it. A
/// worktree under `base` (`.worktrees/` or `.claude/worktrees/`) is `base`
/// itself: sessions run in the project folder.
fn inside<'a>(cwd: &'a str, base: &str) -> Option<&'a str> {
    let rest = Path::new(cwd).strip_prefix(base).ok()?;

    if !rest
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }

    if rest.starts_with(".worktrees") || rest.starts_with(".claude/worktrees") {
        return Some("");
    }

    rest.to_str()
}
