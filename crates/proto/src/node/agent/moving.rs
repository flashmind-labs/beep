//! How far a session's move to this machine has come, as the target says.

use iroh::EndpointId;
use serde::{Deserialize, Serialize};

/// A step of a move, in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovePhase {
    /// The source still runs the turn, or cannot be reached.
    #[default]
    Waiting,
    /// The conversation is being copied.
    Conversation,
    /// The project's files are being brought in line.
    Files,
    /// The session is starting here.
    Resuming,
    /// The session runs here.
    Done,
    /// The move failed; the session stays where it was.
    Failed,
    /// The record was deleted before the session resumed here.
    Cancelled,
}

/// What `SessionsRequest::MoveProgress` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoveProgress {
    pub phase: MovePhase,
    /// Bytes of the conversation copied, and the source's size when known.
    pub copied: u64,
    pub total: u64,
    /// Files of the project not here yet.
    pub files_left: u64,
    /// The source cannot be reached now.
    pub source_away: bool,
    /// Why it failed, when it did.
    pub reason: Option<String>,
}

/// A session of another machine that this node keeps a copy of
/// (`SessionsRequest::Copies`), or a version of a session that lost to
/// another machine's after both were continued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyView {
    pub agent: String,
    /// The agent's own id for the session.
    pub id: String,
    /// The machine that ran it (this one, for a version that lost).
    pub from: EndpointId,
    /// The project, by the name `sync/<name>` has.
    pub project: String,
    /// The folder it ran in, relative to the project's checkout.
    pub path: String,
    /// Unix seconds: its last activity, and when the copy was last brought
    /// up to date.
    pub last_activity: u64,
    pub fetched_at: u64,
    pub size: u64,
    /// Set for a version that lost: when it was set aside.
    pub lost: Option<u64>,
}
