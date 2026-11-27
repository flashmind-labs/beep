//! The harnesses a session can move between, and a transcript as the target
//! machine stores it.

use crate::logs::found::Agent;
use std::path::PathBuf;

/// The harnesses a session can move between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Harness {
    Claude,
    Codex,
    Pi,
    Flsh,
}

impl From<Harness> for Agent {
    fn from(harness: Harness) -> Self {
        match harness {
            Harness::Claude => Self::Claude,
            Harness::Codex => Self::Codex,
            Harness::Pi => Self::Pi,
            Harness::Flsh => Self::Flsh,
        }
    }
}

impl TryFrom<Agent> for Harness {
    type Error = Agent;

    /// The harness for `agent`; `None` for those that keep sessions in
    /// databases.
    fn try_from(agent: Agent) -> Result<Self, Self::Error> {
        match agent {
            Agent::Claude => Ok(Self::Claude),
            Agent::Codex => Ok(Self::Codex),
            Agent::Pi => Ok(Self::Pi),
            Agent::Flsh => Ok(Self::Flsh),
            Agent::OpenCode | Agent::Agy => Err(agent),
        }
    }
}

/// A transcript as the target machine stores it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) bytes: Vec<u8>,
    /// Where it goes, relative to the harness's store: Claude's `projects`
    /// folder, Codex's home, pi's or flsh's sessions folder.
    pub(crate) path: PathBuf,
}
