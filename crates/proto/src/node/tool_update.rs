//! `tool-update/1`: the node installs a newer release of a tool it
//! provisions (`gh`, `teacli`) into its own `bin` folder, checked against
//! the release's published checksums. The server looks up the newest
//! release and names it; the node answers with one `Reply`.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

pub const PROTOCOL: &str = "tool-update/1";

/// A tool the node can update.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Tool {
    Gh,
    Teacli,
}

impl Tool {
    pub const ALL: [Tool; 2] = [Tool::Gh, Tool::Teacli];

    /// Its GitHub repository, which publishes the releases.
    pub fn repo(self) -> &'static str {
        match self {
            Self::Gh => "cli/cli",
            Self::Teacli => "flashmind-labs/teacli",
        }
    }
}

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Installs release `version` (such as `2.103.3`) of `tool`.
    Install { tool: Tool, version: String },
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    Installed {},
    /// In words for the user.
    Failed {
        reason: String,
    },
}
