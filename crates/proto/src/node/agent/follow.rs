//! Read-only session snapshots pushed over an authenticated mesh stream.

use super::{Entry, ExternalSession, SessionInfo};
use serde::{Deserialize, Serialize};

pub const SESSION_FOLLOW_PROTOCOL: &str = "agent-view/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FollowSession {
    pub agent: String,
    pub session: String,
    pub last: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionSource {
    Owned(Box<SessionInfo>),
    External(Box<ExternalSession>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub source: SessionSource,
    /// Initial transcript window, then the suffix to replace at `earlier`.
    pub entries: Vec<Entry>,
    /// Absolute index of the first entry, including entries no longer kept.
    pub earlier: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionUpdate {
    Snapshot(Box<SessionSnapshot>),
    KeepAlive,
    Error { reason: String },
}
