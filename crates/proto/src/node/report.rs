//! `report/1`: what a node shares with a problem report. The desktop sends one
//! `Request` per stream and the node answers with one `Reply`, followed, for a
//! whole log, by a `Reply::Log` for each further part of it.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "report/1";

/// What the desktop sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Only log lines from this Unix time (seconds) on.
    pub since: u64,
    /// The most log bytes wanted, the newest, when `whole` is off.
    pub max_bytes: u32,
    /// The whole log from `since` on, in as many replies as it takes. A node
    /// that predates this gives its newest `max_bytes` in one reply.
    #[serde(default)]
    pub whole: bool,
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
    /// The next part of a whole log, after the `Share` that began it.
    Log { text: String },
}

#[cfg(test)]
#[path = "../tests/node_report.rs"]
mod tests;
