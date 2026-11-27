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
mod harness;
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
pub(crate) use harness::Harness;
pub(crate) use progress::Moves;
pub use read::ALPN as SESSIONS_ALPN;

use super::Agents;
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

/// Copies Claude session `id`'s transcript, kept for the folder `old`, to
/// where Claude Code looks for it in `new`, with the folders in it
/// rewritten: Claude Code finds a session by the folder it runs in. Nothing
/// when there is none, or one is there already. Blocks on the file system.
pub(crate) fn rehome_claude(id: &SessionId, old: &Path, new: &Path) -> Result<()> {
    let projects = crate::logs::claude_dir::config_dir()
        .context("this machine has no home folder")?
        .join("projects");
    let kept_for = |folder: &Path| {
        projects
            .join(crate::logs::claude_dir::slug(folder))
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
