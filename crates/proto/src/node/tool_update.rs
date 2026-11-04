//! `tool-update/1`: the node installs a newer release of a tool it
//! provisions (`gh`, `teacli`, `flsh`) into its own `bin` folder, checked
//! against the release's published checksums. The server looks up the
//! newest release and names it; the node answers with one `Reply`.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

pub const PROTOCOL: &str = "tool-update/1";
/// Offered beside `PROTOCOL` by a node that also installs `Tool::Flsh`; a
/// node from before it cannot read a request that names flsh.
pub const FLSH: &str = "tool-update/flsh";

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
    Flsh,
}

/// Where a tool's releases are published.
pub enum Releases {
    /// This GitHub repository's releases.
    GitHub(&'static str),
    /// `<base>/latest.txt` names the newest version, and
    /// `<base>/<version>/<file>` with `<file>.sha256` holds each file.
    Channel(&'static str),
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Gh, Tool::Teacli, Tool::Flsh];

    pub fn releases(self) -> Releases {
        match self {
            Self::Gh => Releases::GitHub("cli/cli"),
            Self::Teacli => Releases::GitHub("flashmind-labs/teacli"),
            Self::Flsh => Releases::Channel("https://releases.flashmind.com/flashmind/cli"),
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
