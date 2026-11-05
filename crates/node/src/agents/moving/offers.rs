//! What this machine offers other machines for background copies: its
//! sessions of the mesh's projects, active in the last 30 days.

use super::{Harness, read::Offer};
use crate::agents::{
    Agents,
    found::{Agent, within},
};
use beep_mesh::{Key, Mesh};
use beep_proto::node::mesh_state::{StateValue, SyncSpec};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::task::spawn_blocking;

/// How far back sessions are copied.
pub(super) const KEEP_DAYS: u64 = 30;
/// The most sessions one list carries.
const MAX_OFFERS: usize = 2000;

/// A project of the mesh that this machine keeps a checkout of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Project {
    pub(crate) name: String,
    pub(crate) folder: PathBuf,
}

/// The projects of `mesh` this machine keeps a checkout of.
pub(super) fn projects_of(mesh: &Mesh) -> Vec<Project> {
    let me = mesh.me();
    mesh.scan("sync/")
        .into_iter()
        .filter_map(|(key, bytes)| {
            let Key::Sync(name) = key else {
                return None;
            };
            let spec = SyncSpec::from_bytes(&bytes).ok()?;
            let folder = PathBuf::from(spec.folders.get(&me)?);
            Some(Project { name, folder })
        })
        .collect()
}

/// Unix seconds now.
pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Whether a session last active at `last_activity` is young enough to copy.
pub(super) fn is_recent(last_activity: u64, now: u64) -> bool {
    now.saturating_sub(last_activity) <= KEEP_DAYS * 24 * 3600
}

/// The sessions of Claude Code, Codex and pi that ran in a checkout of one
/// of `projects` (or below it) and were active in the last 30 days, newest
/// first.
pub(super) async fn offers(agents: &Agents, projects: &[Project]) -> Vec<Offer> {
    if projects.is_empty() {
        return Vec::new();
    }
    let seen = match agents.sightings().await {
        Ok(seen) => seen,
        Err(e) => {
            log::debug!("sessions not listed for copying: {e:#}");
            return Vec::new();
        }
    };
    let now = now_secs();
    let mut wanted: Vec<_> = seen
        .into_iter()
        .filter(|one| Harness::of(one.agent).is_some() && is_recent(one.last_activity, now))
        .filter_map(|one| {
            let project = projects
                .iter()
                .filter(|project| within(&one.cwd, &project.folder))
                .max_by_key(|project| project.folder.as_os_str().len())?;
            let inside = one.cwd.strip_prefix(&project.folder).ok()?;
            // A worktree folder never syncs, so no other machine could
            // continue a session of one.
            if inside
                .components()
                .next()
                .is_some_and(|first| first.as_os_str() == ".worktrees")
            {
                return None;
            }
            let path = inside.to_str()?.to_owned();
            Some((one, project.name.clone(), path))
        })
        .collect();
    wanted.sort_by_key(|(one, ..)| std::cmp::Reverse(one.last_activity));
    wanted.truncate(MAX_OFFERS);
    let memo = agents.moves.clone_hashes();
    let hashed = spawn_blocking(move || {
        wanted
            .into_iter()
            .filter_map(|(one, project, path)| {
                let file = one.file?;
                let (size, hash) = hash_of(&memo, &file)?;
                Some(Offer {
                    agent: one.agent.slug(),
                    id: one.id,
                    project,
                    path,
                    last_activity: one.last_activity,
                    size,
                    hash,
                })
            })
            .collect::<Vec<_>>()
    })
    .await;
    hashed.unwrap_or_default()
}

/// A file's size and blake3 hash, read again only when its size or time
/// changed.
fn hash_of(memo: &super::progress::Hashes, file: &Path) -> Option<(u64, Vec<u8>)> {
    let meta = std::fs::metadata(file).ok()?;
    let (size, modified) = (meta.len(), meta.modified().ok()?);
    if size > super::read::MAX_TRANSCRIPT {
        return None;
    }
    if let Some(hash) = memo.get(file, size, modified) {
        return Some((size, hash));
    }
    let mut hasher = blake3::Hasher::new();
    let mut reader = File::open(file).ok()?.take(size);
    let mut buf = vec![0; 256 * 1024];
    loop {
        let got = reader.read(&mut buf).ok()?;
        if got == 0 {
            break;
        }
        hasher.update(&buf[..got]);
    }
    let hash = hasher.finalize().as_bytes().to_vec();
    memo.put(file, size, modified, hash.clone());
    Some((size, hash))
}

/// The harness behind an agent slug the peer named, when sessions of it
/// are copied.
pub(super) fn harness_of(slug: &str) -> Option<Harness> {
    Agent::from_name(slug).and_then(Harness::of)
}
