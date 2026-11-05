//! Moving a session between machines: what a harness stores for a session
//! once its folder changed (pure functions over a transcript's bytes), and
//! reading a transcript from the machine that ran it (`beep-sessions/1`).

mod alpn;
mod claude;
mod codex;
mod folder;
mod json;
mod pi;
mod read;
mod store;

#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod tests;

pub use alpn::SessionsAlpn;
pub(crate) use read::ALPN as SESSIONS_ALPN;

use super::found::Agent;
use std::path::{Path, PathBuf};

/// The harnesses a session can move between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Harness {
    Claude,
    Codex,
    Pi,
}

impl Harness {
    /// The harness for `agent`; `None` for those that keep sessions in a
    /// database.
    pub(crate) fn of(agent: Agent) -> Option<Self> {
        match agent {
            Agent::Claude => Some(Self::Claude),
            Agent::Codex => Some(Self::Codex),
            Agent::Pi => Some(Self::Pi),
            Agent::Flsh | Agent::OpenCode => None,
        }
    }
}

/// A transcript as the target machine stores it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) bytes: Vec<u8>,
    /// Where it goes, relative to the harness's store: Claude's `projects`
    /// folder, Codex's home, pi's sessions folder.
    pub(crate) path: PathBuf,
}

/// `transcript` of session `id`, written for a session that worked in
/// `old` and now works in `new`. `file_name` is the name the source kept
/// it under (Codex and pi put the start time in it). Only the folder
/// fields change; every other byte stays.
pub(crate) fn rewrite(
    harness: Harness,
    transcript: &[u8],
    id: &str,
    file_name: &str,
    old: &Path,
    new: &Path,
) -> Stored {
    match harness {
        Harness::Claude => claude::rewrite(transcript, id, old, new),
        Harness::Codex => codex::rewrite(transcript, file_name, old, new),
        Harness::Pi => pi::rewrite(transcript, file_name, old, new),
    }
}
