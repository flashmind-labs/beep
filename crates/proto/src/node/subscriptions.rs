//! One topic subscription per stream on a dedicated peer connection.

use super::events::{Query, Reply as EventsReply, Snapshot, Topic};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "peer-subscriptions/1";

pub const ALPN: &[u8] = b"beep-peer/subs/1";

/// Sent first so the peer can accept the QUIC stream before its first watch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Open {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    Watch {
        id: u64,
        topic: Topic,
        queries: Vec<Query>,
        /// The follower applies `Value::SessionsDelta`: the node sends the
        /// `agents` list once whole, then only the rows that changed. A node
        /// from before it ignores the flag and keeps sending whole lists.
        #[serde(default)]
        deltas: bool,
    },
    Refresh {
        id: u64,
    },
    Ping {
        id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    Ready,
    Watching {
        id: u64,
    },
    Snapshot(Box<Snapshot>),
    SnapshotError {
        query: Option<u64>,
        topic: Topic,
        reason: String,
    },
    Error {
        id: u64,
        reason: String,
    },
    Pong {
        id: u64,
    },
}

// The inbound events/1 protocol remains available to released apps.
impl From<Reply> for EventsReply {
    fn from(reply: Reply) -> Self {
        match reply {
            Reply::Ready => EventsReply::Ready,
            Reply::Watching { id } => EventsReply::Watching { id },
            Reply::Snapshot(snapshot) => EventsReply::Snapshot(snapshot),
            Reply::SnapshotError {
                query,
                topic,
                reason,
            } => EventsReply::SnapshotError {
                query,
                topic,
                reason,
            },
            Reply::Error { id, reason } => EventsReply::Error { id, reason },
            Reply::Pong { id } => EventsReply::Pong { id },
        }
    }
}
