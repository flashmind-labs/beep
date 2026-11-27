//! Correlated actions on a dedicated peer connection.

use super::events::{CallReply, Operation};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "peer-commands/1";

pub const ALPN: &[u8] = b"beep-peer/cmd/1";

/// The first frame selects correlated calls or an existing session exchange.
/// Session bodies retain their own request and reply schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Open {
    Calls,
    Session {
        protocol: String,
        /// Requests negotiated zstd frame compression for replies from the node.
        #[serde(default)]
        compress: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    Call { id: u64, operation: Box<Operation> },
    Ping { id: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    Ready,
    Result { id: u64, reply: Box<CallReply> },
    Error { id: u64, reason: String },
    Pong { id: u64 },
}
