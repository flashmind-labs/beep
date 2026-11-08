//! `report/1`: what a node shares with a problem report. The desktop sends one
//! `Request` per stream and the node answers with one `Reply`.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "report/1";
/// The most log bytes a node gives.
pub const MAX_BYTES: u32 = 1024 * 1024;

/// What the desktop sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Only log lines from this Unix time (seconds) on.
    pub since: u64,
    /// The most log bytes wanted, up to `MAX_BYTES`.
    pub max_bytes: u32,
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// What the node shares, nothing else leaves it.
    Share {
        log: String,
        status: String,
        version: String,
        os: String,
        os_version: String,
    },
    /// It cannot share, in words for the user.
    Failed { reason: String },
}

#[cfg(test)]
#[path = "../tests/node_report.rs"]
mod tests;
