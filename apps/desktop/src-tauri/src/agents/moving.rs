//! Moving a coding agent session to another machine: the desktop writes the
//! mesh's move record, takes it back, lists the records and asks the target
//! how the move goes. `move-state.ts` shows the answers.

use super::sessions::{MoveView, moves};
use crate::store::lock;
use crate::{
    error::{DesktopError, Result},
    fleet::{
        Machines, agents as direct,
        projects::{FleetState, desk},
        view_signal::View,
    },
};
use beep_fleet::{ChatMoveAsk, ChatSession, MoveAsk, MoveState};
use beep_proto::SessionId;
use beep_proto::{
    ApiError, ModelId,
    node::agent::{MoveEntry, MovePhase, MoveProgress},
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::SystemTime,
};

/// How the target says a move stands.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MoveProgressView {
    /// `known`, `unknown` (the target has heard of no such move) or `update`
    /// (the target is from before moves).
    pub answer: &'static str,
    /// `waiting`, `conversation`, `files`, `resuming`, `done`, `failed` or
    /// `cancelled`; `waiting` when the answer is not `known`.
    pub phase: &'static str,
    pub copied: u64,
    pub total: u64,
    pub source_away: bool,
    pub reason: Option<String>,
    /// The worktree coming over, and which of how many.
    pub worktree: Option<String>,
    pub worktree_at: u32,
    pub worktrees: u32,
    pub edits_left: bool,
    /// Bytes of a chat's files copied, and what they take on the source.
    pub files_copied: u64,
    pub files_total: u64,
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
            source_away: progress.source_away,
            reason: progress.reason,
            worktree: progress.worktree,
            worktree_at: progress.worktree_at,
            worktrees: progress.worktrees,
            edits_left: progress.edits_left,
            files_copied: progress.files_copied,
            files_total: progress.files_total,
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
    machines: Machines<'_>,
    machine_id: String,
    to: String,
    agent: String,
    session: SessionId,
    project: String,
    cwd: String,
    mode: Option<String>,
    model: Option<String>,
    stop: bool,
) -> Result<MoveView> {
    let from = machine(&machine_id)?;
    // The worktrees it created go along in the record, so the target keeps them.
    let worktrees = direct::created_worktrees(&machines, from, &agent, &session).await?;
    let ask = MoveAsk {
        agent: &agent,
        id: &session,
        from,
        to: machine(&to)?,
        project: &project,
        cwd: &cwd,
        mode,
        model: model.map(ModelId::new),
        worktrees,
        stop,
    };
    Ok(desk(&fleet).await?.move_session(ask).await?.into())
}

/// A session of a chat to move, as the window lists it.
#[derive(Debug, Deserialize)]
pub struct ChatSessionAsk {
    agent: String,
    /// The agent's own id.
    session: SessionId,
    cwd: String,
    model: Option<String>,
    ended: bool,
}

/// Asks `to` to take over the chat that runs on `machine_id`, with its
/// threads and its folder: `sessions` is the chat, then its threads. Refused
/// in plain words when one of them, or one of the two nodes, cannot do it.
#[tauri::command]
pub async fn chat_move(
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    machine_id: String,
    to: String,
    sessions: Vec<ChatSessionAsk>,
    stop: bool,
) -> Result<Vec<MoveView>> {
    let (from, to) = (machine(&machine_id)?, machine(&to)?);

    if !direct::moves_chats(&machines, from) || !direct::moves_chats(&machines, to) {
        return Err(DesktopError::from(ApiError::bad_request(
            "Update beep-node on both machines to move a chat.",
        )));
    }

    let ask = ChatMoveAsk {
        from,
        to,
        sessions: sessions
            .into_iter()
            .map(|session| ChatSession {
                agent: session.agent,
                id: session.session,
                cwd: session.cwd,
                model: session.model.map(ModelId::new),
                ended: session.ended,
            })
            .collect(),
        stop,
    };
    let records = desk(&fleet).await?.move_chat(ask).await?;
    Ok(records.into_iter().map(Into::into).collect())
}

/// Whether the node of `machine_id` takes part in moves; one from before
/// them would ignore a move and leave the session where it is.
#[tauri::command]
pub async fn session_move_probe(fleet: FleetState<'_>, machine_id: String) -> Result<bool> {
    let machine = machine(&machine_id)?;
    Ok(desk(&fleet).await?.takes_moves(machine).await?)
}

/// Takes back the move of `session`, unless it already resumed on the target.
#[tauri::command]
pub async fn session_move_cancel(fleet: FleetState<'_>, session: SessionId) -> Result<()> {
    Ok(desk(&fleet).await?.cancel_move(&session).await?)
}

/// Records older than this are not asked about.
const RECENT_MS: u64 = 24 * 3600 * 1000;
/// The most moves one view tells the progress of.
const MAX_ASKED: usize = 8;

/// The last answer of each move that ended, by session and record: it does not change again.
static ENDED: LazyLock<Mutex<HashMap<(SessionId, u64), MoveProgressView>>> =
    LazyLock::new(Mutex::default);

/// What a target that pushes its moves says of `record`: `unknown` when its push has no entry.
/// `None` for a target that pushed nothing, which is asked.
fn pushed_progress(
    record: &MoveView,
    pushed: &HashMap<EndpointId, Vec<MoveEntry>>,
) -> Option<MoveProgressView> {
    let target: EndpointId = record.to.parse().ok()?;
    let entries = pushed.get(&target)?;
    let state = entries
        .iter()
        .find(|entry| entry.session == record.session && entry.at == record.at)
        .map_or(MoveState::Unknown, |entry| {
            MoveState::Known(entry.progress.clone())
        });

    Some(state.into())
}

/// How the target says each recent move of `moves` stands, for the view beside the moves: from
/// what the targets pushed in `pushed`, and asked only of a target that pushed nothing. A
/// target's push draws the view again.
pub(crate) async fn progress_views(
    session: &Arc<beep_fleet::Session>,
    moves: &[MoveView],
    pushed: &HashMap<EndpointId, Vec<MoveEntry>>,
) -> Vec<View> {
    let Ok(desk) = desk(session).await else {
        return Vec::new();
    };
    let now = crate::beep::epoch_ms(SystemTime::now());
    let mut recent: Vec<&MoveView> = moves
        .iter()
        .filter(|record| now.saturating_sub(record.at) < RECENT_MS)
        .collect();
    recent.sort_by_key(|record| std::cmp::Reverse(record.at));
    recent.truncate(MAX_ASKED);
    let desk = &desk;
    let reads = recent.into_iter().map(|record| async move {
        let args = json!({ "session": record.session });
        let key = (record.session.clone(), record.at);

        if let Some(progress) = pushed_progress(record, pushed) {
            return View::new("session_move_progress", args, progress);
        }

        if let Some(ended) = lock(&ENDED).get(&key) {
            return View::new("session_move_progress", args, ended);
        }

        match desk.move_state(&record.session).await {
            Ok(state) => {
                let progress = MoveProgressView::from(state);

                if progress.answer == "known"
                    && matches!(progress.phase, "done" | "failed" | "cancelled")
                {
                    lock(&ENDED).insert(key, progress.clone());
                }

                View::new("session_move_progress", args, progress)
            }
            Err(error) => Some(View::failed(
                "session_move_progress",
                args,
                &error.to_string(),
            )),
        }
    });

    futures::future::join_all(reads)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// The moves in the mesh: the latest record of each session.
#[tauri::command]
pub async fn session_moves(fleet: FleetState<'_>) -> Result<Vec<MoveView>> {
    moves(fleet).await
}

/// How the move of `session` stands, as its target says.
#[tauri::command]
pub async fn session_move_progress(
    fleet: FleetState<'_>,
    session: SessionId,
) -> Result<MoveProgressView> {
    Ok(desk(&fleet).await?.move_state(&session).await?.into())
}

#[cfg(test)]
#[path = "../tests/agents_moving.rs"]
mod tests;
