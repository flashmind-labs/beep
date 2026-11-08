//! Read-only session snapshots pushed over an authenticated mesh stream.

use super::{Entry, ExternalSession, SessionInfo};
use crate::{AgentSlug, SessionId};
use serde::{Deserialize, Serialize};

pub const SESSION_FOLLOW_PROTOCOL: &str = "agent-view/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FollowSession {
    pub agent: AgentSlug,
    pub session: SessionId,
    pub last: u32,
    #[serde(default)]
    pub after: Option<SessionCursor>,
}

/// A checkpoint for the retained transcript window, not a replay-log position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCursor {
    /// First entry included in the cached window's fingerprint.
    pub start: u64,
    /// Position after its last entry. Mutable entries can change without advancing it.
    pub next: u64,
    /// BLAKE3 of canonical entries in `[start, next)`.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionSource {
    Owned(Box<SessionInfo>),
    External(Box<ExternalSession>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub source: SessionSource,
    /// Replace the transcript suffix beginning at `earlier` with these entries.
    pub entries: Vec<Entry>,
    /// Absolute index of the first entry, including entries no longer kept.
    pub earlier: u64,
    pub cursor: SessionCursor,
    /// The initial snapshot validated `after`; keep its cached prefix.
    pub resumed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionUpdate {
    /// Replace cached state unless `resumed` validates and preserves its prefix.
    Snapshot(Box<SessionSnapshot>),
    /// Replace from `earlier` through the old end, including an empty truncation.
    Delta(Box<SessionSnapshot>),
    KeepAlive,
    Error {
        reason: String,
    },
}
