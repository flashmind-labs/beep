//! The watchers of the agents' stores and of the sessions started outside
//! Beep: what `Agents` keeps while some client follows agent topics.

use crate::occupancy::Watching;
use crate::logs::found::Agent;
use beep_proto::SessionId;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{sync::mpsc, time::Instant};

pub(crate) struct Sources {
    pub(super) paths: crate::events::paths::Paths,
    pub(super) processes: std::sync::Mutex<std::collections::HashMap<(String, SessionId), ProcessWatch>>,
    pub(super) pending: Arc<Mutex<Pending>>,
    pub(super) dirty: mpsc::Sender<()>,
    /// Tells the occupancy the stores are read as they change.
    pub(super) _watching: Watching,
    pub(super) _summary: tokio_util::task::AbortOnDropHandle<()>,
}

#[derive(Default)]
pub(super) struct Pending {
    pub(super) agents: BTreeSet<Agent>,
}

impl Pending {
    pub(super) fn record(&mut self, roots: &[(Agent, PathBuf)], changed: &[PathBuf]) {
        self.agents.extend(
            roots
                .iter()
                .filter_map(|(agent, path)| changed.contains(path).then_some(*agent)),
        );
    }

    /// Records every agent whose watched root saw a relevant file change.
    /// Returns `false` when every reported path is noise (logs, `-shm`, empty `-wal`).
    pub(super) fn record_paths(
        &mut self,
        roots: &[(Agent, PathBuf)],
        changed_roots: &[PathBuf],
        changed_paths: Option<&[PathBuf]>,
    ) -> bool {
        let Some(paths) = changed_paths else {
            self.record(roots, changed_roots);
            return !changed_roots.is_empty();
        };

        let mut matched = false;
        for (agent, root) in roots {
            if changed_roots.contains(root)
                && paths.iter().any(|path| relevant_path(*agent, root, path))
            {
                self.agents.insert(*agent);
                matched = true;
            }
        }
        matched
    }
}

fn non_empty_wal(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.len() > 0)
}

/// Whether a filesystem event at `path` under `root` can change `agent`'s
/// sessions or spend. Skips logs, SQLite `-shm` sidecars and empty `-wal`
/// files created when a CLI or reader opens a WAL database.
fn relevant_path(agent: Agent, root: &Path, path: &Path) -> bool {
    if path == root {
        return true;
    }

    if root.starts_with(path) {
        return root.exists();
    }

    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if name.ends_with("-shm") {
        return false;
    }

    match agent {
        Agent::Claude => name.ends_with(".jsonl") || name.ends_with(".json"),
        Agent::Codex | Agent::Pi => name.ends_with(".jsonl"),
        Agent::Flsh => {
            name.ends_with(".jsonl")
                || name.ends_with(".db")
                || (name.ends_with(".db-wal") && non_empty_wal(path))
                || name.ends_with(".openrouter.json")
                || name == ".sqlite-imported"
        }
        Agent::OpenCode => {
            relative.components().count() == 1
                && (name.ends_with(".db")
                    || (name.ends_with(".db-wal") && non_empty_wal(path))
                    || name == "auth.json")
        }
        Agent::Agy => {
            relative.components().count() == 1
                && (name.ends_with(".db") || (name.ends_with(".db-wal") && non_empty_wal(path)))
        }
    }
}

#[derive(PartialEq, Eq)]
pub(super) enum Observation {
    Process(u32),
    Recent(u64),
}

pub(super) struct ProcessWatch {
    pub(super) revision: Observation,
    pub(super) seen: Instant,
    pub(super) cwd: PathBuf,
    pub(super) _task: tokio_util::task::AbortOnDropHandle<()>,
}

pub(super) fn active_roots(
    processes: &std::collections::HashMap<(String, SessionId), ProcessWatch>,
) -> Vec<PathBuf> {
    let mut roots: Vec<_> = processes
        .values()
        .filter(|held| {
            matches!(held.revision, Observation::Process(_)) && !held._task.is_finished()
        })
        .map(|held| held.cwd.clone())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

// Both log readers classify an unowned write as recent for three minutes.
// Schedule that single state transition, rather than periodically rereading idle logs.
pub(super) fn recent_deadline(session: &beep_proto::node::agent::ExternalSession) -> Option<Instant> {
    if session.pid.is_some() || session.presence != beep_proto::node::agent::Presence::Recent {
        return None;
    }

    let remaining = session
        .last_activity
        .saturating_add(181)
        .saturating_sub(crate::logs::unix_now());
    Instant::now().checked_add(std::time::Duration::from_secs(remaining.max(1)))
}
