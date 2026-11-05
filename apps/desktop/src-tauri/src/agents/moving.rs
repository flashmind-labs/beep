//! Moving a coding agent session to another machine: the desktop writes the
//! mesh's move record, takes it back, lists the records and asks the target
//! how the move goes. `move-state.ts` shows the answers.

use crate::{
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk},
};
use beep_fleet::{AskedWorktree, MoveAsk, MoveState, WorktreeMove};
use beep_proto::{
    ApiError,
    node::{
        agent::{MovePhase, MoveProgress},
        mesh_state::SessionMove,
    },
};
use iroh::EndpointId;
use serde::Serialize;

/// A move record as the window shows it.
#[derive(Debug, Serialize, PartialEq)]
pub struct MoveView {
    /// The agent's own id for the session.
    pub session: String,
    pub agent: String,
    /// The machine that ran it, and the one that takes it over.
    pub from: String,
    pub to: String,
    pub project: String,
    /// Milliseconds since the Unix epoch.
    pub at: u64,
}

impl From<SessionMove> for MoveView {
    fn from(record: SessionMove) -> Self {
        Self {
            session: record.id,
            agent: record.agent.to_string(),
            from: record.from.to_string(),
            to: record.to.to_string(),
            project: record.project,
            at: record.at,
        }
    }
}

/// How the target says a move stands.
#[derive(Debug, Serialize, PartialEq)]
pub struct MoveProgressView {
    /// `known`, `unknown` (the target has heard of no such move) or `update`
    /// (the target is from before moves).
    pub answer: &'static str,
    /// `waiting`, `conversation`, `files`, `resuming`, `done`, `failed` or
    /// `cancelled`; `waiting` when the answer is not `known`.
    pub phase: &'static str,
    pub copied: u64,
    pub total: u64,
    pub files_left: u64,
    pub source_away: bool,
    pub reason: Option<String>,
}

impl From<MoveState> for MoveProgressView {
    fn from(state: MoveState) -> Self {
        let (answer, progress) = match state {
            MoveState::Known(progress) => ("known", progress),
            MoveState::Unknown => ("unknown", MoveProgress::default()),
            MoveState::NeedsUpdate => ("update", MoveProgress::default()),
        };
        Self {
            answer,
            phase: phase_name(progress.phase),
            copied: progress.copied,
            total: progress.total,
            files_left: progress.files_left,
            source_away: progress.source_away,
            reason: progress.reason,
        }
    }
}

fn phase_name(phase: MovePhase) -> &'static str {
    match phase {
        MovePhase::Waiting => "waiting",
        MovePhase::Conversation => "conversation",
        MovePhase::Files => "files",
        MovePhase::Resuming => "resuming",
        MovePhase::Done => "done",
        MovePhase::Failed => "failed",
        MovePhase::Cancelled => "cancelled",
    }
}

fn machine(id: &str) -> Result<EndpointId> {
    id.parse().map_err(|_| {
        DesktopError::from(ApiError::bad_request("That is not a machine of the mesh."))
    })
}

/// Asks `to` to take over session `session` of `agent`, which runs on
/// `machine_id` in `cwd`. `stop` ends the running turn instead of waiting
/// for it. Refused in plain words when the session cannot move yet.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn session_move(
    fleet: FleetState<'_>,
    machine_id: String,
    to: String,
    agent: String,
    session: String,
    project: String,
    cwd: String,
    worktree_folder: Option<String>,
    worktree_branch: Option<String>,
    mode: Option<String>,
    stop: bool,
) -> Result<MoveView> {
    let ask = MoveAsk {
        agent: &agent,
        id: &session,
        from: machine(&machine_id)?,
        to: machine(&to)?,
        project: &project,
        cwd: &cwd,
        worktree: worktree_folder.as_deref().map(|folder| AskedWorktree {
            folder,
            branch: worktree_branch.as_deref().unwrap_or_default(),
        }),
        mode,
        stop,
    };
    Ok(desk(&fleet).await?.move_session(ask).await?.into())
}

/// Whether the node of `machine_id` takes part in moves; one from before
/// them would ignore a move and leave the session where it is.
#[tauri::command]
pub async fn session_move_probe(fleet: FleetState<'_>, machine_id: String) -> Result<bool> {
    let machine = machine(&machine_id)?;
    Ok(desk(&fleet).await?.takes_moves(machine).await?)
}

/// Whether the node of `machine_id` can take a session working in a worktree
/// on `branch` of `project`: `takes`, `ignored` (its Syncthing is set not to
/// sync the worktree) or `update` (its node is from before worktree moves).
#[tauri::command]
pub async fn session_move_worktree_probe(
    fleet: FleetState<'_>,
    machine_id: String,
    project: String,
    branch: String,
) -> Result<&'static str> {
    let machine = machine(&machine_id)?;
    Ok(
        match desk(&fleet)
            .await?
            .worktree_move(machine, &project, &branch)
            .await?
        {
            WorktreeMove::Takes => "takes",
            WorktreeMove::Ignored => "ignored",
            WorktreeMove::NeedsUpdate => "update",
        },
    )
}

/// Takes back the move of `session`, unless it already resumed on the target.
#[tauri::command]
pub async fn session_move_cancel(fleet: FleetState<'_>, session: String) -> Result<()> {
    Ok(desk(&fleet).await?.cancel_move(&session).await?)
}

/// The moves in the mesh: the latest record of each session.
#[tauri::command]
pub async fn session_moves(fleet: FleetState<'_>) -> Result<Vec<MoveView>> {
    Ok(desk(&fleet)
        .await?
        .moves()
        .into_iter()
        .map(Into::into)
        .collect())
}

/// How the move of `session` stands, as its target says.
#[tauri::command]
pub async fn session_move_progress(
    fleet: FleetState<'_>,
    session: String,
) -> Result<MoveProgressView> {
    Ok(desk(&fleet).await?.move_state(&session).await?.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_from_before_moves_says_to_update() {
        let view = MoveProgressView::from(MoveState::NeedsUpdate);
        assert_eq!((view.answer, view.phase), ("update", "waiting"));
    }

    #[test]
    fn a_known_move_carries_its_numbers_and_reason() {
        let view = MoveProgressView::from(MoveState::Known(MoveProgress {
            phase: MovePhase::Failed,
            copied: 4,
            total: 6,
            files_left: 12,
            source_away: false,
            reason: Some("no checkout here".into()),
        }));
        assert_eq!(
            (view.answer, view.phase, view.files_left),
            ("known", "failed", 12)
        );
        assert_eq!(view.reason.as_deref(), Some("no checkout here"));
    }
}
