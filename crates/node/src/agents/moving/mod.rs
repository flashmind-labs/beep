//! Moving a session between machines: what a harness stores for a session
//! once its folder changed (pure functions over a transcript's bytes), and
//! reading a transcript from the machine that ran it (`beep-sessions/1`).

mod alpn;
mod claude;
mod codex;
mod dial;
mod folder;
mod follow;
mod json;
mod pi;
mod progress;
mod read;
mod saved;
mod source;
mod store;
mod target;

#[cfg(all(test, unix))]
mod move_tests;
#[cfg(test)]
mod read_tests;
#[cfg(test)]
mod tests;

pub use alpn::SessionsAlpn;
pub(crate) use follow::follow;
pub(crate) use progress::Moves;
pub(crate) use read::ALPN as SESSIONS_ALPN;

use super::{Agents, found::Agent};
use crate::files::sync::Syncs;
use iroh::{Endpoint, EndpointId};
use saved::Folder;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use store::StoreRoots;

/// The most moves one node takes part in at once.
const MAX_MOVES: usize = 8;

/// What the moves of this node share.
struct Env {
    agents: Arc<Agents>,
    mesh: beep_mesh::Mesh,
    endpoint: Endpoint,
    /// The synced projects, once this node shares folders.
    syncs: Option<Syncs>,
    state: Folder,
    roots: StoreRoots,
}

impl Env {
    /// What the mesh calls `device`, for a line in a session.
    fn name_of(&self, device: &EndpointId) -> String {
        self.mesh
            .devices()
            .into_iter()
            .find(|status| status.device.id == *device)
            .map_or_else(
                || device.fmt_short().to_string(),
                |status| status.device.name,
            )
    }
}

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
