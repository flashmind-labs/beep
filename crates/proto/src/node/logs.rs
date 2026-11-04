//! `logs/1`: the node's recent log. The server sends one `Request` per stream
//! and the node answers with one `Reply`.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "logs/1";
/// The most lines a node gives.
pub const MAX_LINES: u32 = 1000;
/// The most bytes a node gives, whole lines from the end.
pub const MAX_BYTES: usize = 256 * 1024;

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// The last this many lines, up to `MAX_LINES`.
    pub lines: u32,
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// The lines, oldest first; empty when nothing was logged lately.
    Log { text: String },
    /// It has no log to read, in words for the user.
    Failed { reason: String },
}
