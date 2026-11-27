//! Read-only session snapshots pushed over an authenticated mesh stream.

use super::{Entry, ExternalSession, SessionInfo};
use crate::{AgentSlug, SessionId};
use serde::{Deserialize, Serialize};

pub const SESSION_FOLLOW_PROTOCOL: &str = "agent-view/1";

/// The `SessionUpdate::Error` reason a node gives when it has as many session
/// readers as it allows. Released apps match this text; keep it unchanged.
pub const FOLLOW_READERS_FULL: &str = "too many session readers";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FollowSession {
    pub agent: AgentSlug,
    pub session: SessionId,
    pub last: u32,
    #[serde(default)]
    pub after: Option<SessionCursor>,
    /// Client accepts `SessionUpdate::Patch`, prefix-validated tail resumes,
    /// and may send `FollowControl` frames after the node sets `controls: true`.
    #[serde(default)]
    pub patches: bool,
    /// Initial visibility when `patches` is true.
    #[serde(default)]
    pub shown: bool,
}

/// Client-to-node control message on `agent-view/1`, sent only when the node
/// acknowledged control support on its initial snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FollowControl {
    /// Whether the followed session is currently visible on screen.
    Shown(bool),
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
    /// Position after the settled prefix `[start, prefix_next)` (typically `next - 1`),
    /// allowing reconnect resume when only the trailing streaming entry mutated.
    #[serde(default)]
    pub prefix_next: u64,
    /// BLAKE3 of canonical entries in `[start, prefix_next)`, empty when absent.
    #[serde(default)]
    pub prefix_fingerprint: String,
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
    /// The node accepts `FollowControl` messages on this stream.
    #[serde(default)]
    pub controls: bool,
}

/// One incremental mutation against the client's retained transcript entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EntryPatch {
    /// Appends `text` to the existing entry at `index`; all other fields are unchanged.
    AppendText { index: u64, text: String },
    /// Replaces an existing entry or appends at the end.
    Replace { index: u64, entry: Entry },
    /// Truncates the transcript to end at `next`.
    Truncate { next: u64 },
}

/// Incremental transcript update that avoids re-sending unchanged source metadata
/// and already-transmitted entry text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPatch {
    /// Present only when `SessionSource` changed since the previous update.
    pub source: Option<SessionSource>,
    pub cursor: SessionCursor,
    pub ops: Vec<EntryPatch>,
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
    /// Incremental entry patches and optional source metadata for `patches: true` clients.
    Patch(Box<SessionPatch>),
}
