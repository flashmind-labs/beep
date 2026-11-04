//! `search/1`: where the node looks for git checkouts (`search` in its
//! `node.toml`). The server sends one `Request` per stream and the node
//! answers with one `Reply`.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "search/1";

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    Get {},
    /// Looks in `folder` (empty for home) from now on, and walks it again
    /// in the background.
    Set {
        folder: String,
    },
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// The folder as written, `~/` for home.
    Folder { folder: String },
    /// In words for the user.
    Failed { reason: String },
}
