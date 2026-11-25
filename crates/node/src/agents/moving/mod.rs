//! Moving a session between machines: what a harness stores for a session
//! once its folder changed (pure functions over a transcript's bytes), and
//! reading a transcript from the machine that ran it (`beep-sessions/1`).

mod alpn;
mod claude;
mod codex;
mod dial;
mod files;
mod flsh;
mod folder;
mod follow;
mod json;
mod pi;
mod progress;
mod read;
mod resume;
mod saved;
mod source;
mod store;
mod target;
mod worktree;

#[cfg(all(test, unix))]
#[path = "../../tests/agents_moving_move_tests.rs"]
mod move_tests;
#[cfg(test)]
#[path = "../../tests/agents_moving_read_tests.rs"]
mod read_tests;
#[cfg(test)]
#[path = "../../tests/agents_moving.rs"]
mod tests;

pub use alpn::SessionsAlpn;
pub(crate) use folder::project_folder;
pub(crate) use follow::follow;
pub(crate) use progress::Moves;
pub use read::ALPN as SESSIONS_ALPN;

use super::{Agents, found::Agent};
use anyhow::{Context, Result};
use beep_proto::SessionId;
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
    state: Folder,
    roots: StoreRoots,
    /// This machine's home folder, which holds its chats.
    home: PathBuf,
}

impl Env {
    /// What the mesh calls `device`, for a line in a session.
    fn name_of(&self, device: &EndpointId) -> String {
        device_name(&self.mesh, device)
    }
}

pub(super) fn device_name(mesh: &beep_mesh::Mesh, device: &EndpointId) -> String {
    mesh.devices()
        .into_iter()
        .find(|status| status.device.id == *device)
        .map_or_else(
            || device.fmt_short().to_string(),
            |status| status.device.name,
        )
}

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

/// Copies Claude session `id`'s transcript, kept for the folder `old`, to
/// where Claude Code looks for it in `new`, with the folders in it
/// rewritten: Claude Code finds a session by the folder it runs in. Nothing
/// when there is none, or one is there already. Blocks on the file system.
pub(crate) fn rehome_claude(id: &SessionId, old: &Path, new: &Path) -> Result<()> {
    let projects = super::claude_dir::config_dir()
        .context("this machine has no home folder")?
        .join("projects");
    let kept_for = |folder: &Path| {
        projects
            .join(super::claude_dir::slug(folder))
            .join(format!("{id}.jsonl"))
    };
    let from = kept_for(old);

    if !from.is_file() || kept_for(new).exists() {
        return Ok(());
    }

    let folders = folder::Folders {
        old: old.to_owned(),
        new: new.to_owned(),
    };
    let stored = claude::rewrite(&std::fs::read(&from)?, id, &folders);
    store::write(&projects.join(&stored.path), &stored)
}

/// A transcript as the target machine stores it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) bytes: Vec<u8>,
    /// Where it goes, relative to the harness's store: Claude's `projects`
    /// folder, Codex's home, pi's or flsh's sessions folder.
    pub(crate) path: PathBuf,
}

/// `transcript` of session `id`, written for a session that worked in
/// `folders.old` and now works in `folders.new`. `file_name` is the name the source kept
/// it under (Codex and pi put the start time in it). Only the folder
/// fields change; every other byte stays.
fn rewrite(
    harness: Harness,
    transcript: &[u8],
    id: &SessionId,
    file_name: &str,
    folders: &folder::Folders,
) -> Stored {
    match harness {
        Harness::Claude => claude::rewrite(transcript, id, folders),
        Harness::Codex => codex::rewrite(transcript, file_name, folders),
        Harness::Pi => pi::rewrite(transcript, file_name, folders),
        Harness::Flsh => flsh::rewrite(transcript, id, folders),
    }
}
