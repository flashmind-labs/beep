//! The desktop's copy of each machine's search index, one file per machine. `palette::copies`
//! keeps them current; the palette and the agents search them.

use crate::store::lock;
use beep_node::agents::search::IndexCopy;
use beep_proto::{
    SessionId,
    node::agent::{SearchHit, SearchSession},
};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};
use tauri::{AppHandle, Manager};
use tokio::task::spawn_blocking;

pub(crate) struct Copies {
    dir: PathBuf,
    held: Mutex<HashMap<EndpointId, Arc<Mutex<IndexCopy>>>>,
}

impl Copies {
    /// Opens the copies kept under the app's `data` folder, so offline
    /// machines are searched too.
    pub(crate) fn load(data: &Path) -> Self {
        // The index this desktop built itself before nodes kept their own.
        for old in ["search.sqlite", "search.sqlite-wal", "search.sqlite-shm"] {
            let _ = fs::remove_file(data.join(old));
        }

        let copies = Self {
            dir: data.join("search-copies"),
            held: Mutex::default(),
        };
        let kept = fs::read_dir(&copies.dir).into_iter().flatten().flatten();

        for machine in kept.filter_map(|entry| machine_of(&entry.path())) {
            if let Err(e) = copies.copy(machine) {
                log::warn!("dropping the search copy of {machine}: {e:#}");
                let _ = fs::remove_file(copies.path(machine));
            }
        }

        copies
    }

    /// The hits of each copy, with the machine each came from.
    pub(crate) fn hits(&self, query: &str) -> Vec<(EndpointId, SearchHit)> {
        let held: Vec<_> = self
            .held()
            .iter()
            .map(|(id, copy)| (*id, copy.clone()))
            .collect();
        let mut found = Vec::new();

        for (machine, copy) in held {
            // One copy that cannot be read leaves out only its machine.
            match lock(&copy).find(query, &[]) {
                Ok(hits) => found.extend(hits.into_iter().map(|hit| (machine, hit))),
                Err(e) => log::warn!("searching the copy of {machine}: {e:#}"),
            }
        }

        found
    }

    /// Looks up one session across the kept copies (or `machine` alone).
    /// Prefers `prefer` when several machines hold it, else newest activity.
    fn find_session(
        &self,
        id: &SessionId,
        machine: Option<EndpointId>,
        prefer: Option<EndpointId>,
    ) -> Option<(EndpointId, SessionId, u64, SearchSession)> {
        let held: Vec<_> = self
            .held()
            .iter()
            .filter(|(peer, _)| machine.is_none_or(|wanted| **peer == wanted))
            .map(|(peer, copy)| (*peer, copy.clone()))
            .collect();
        let mut best: Option<(EndpointId, SessionId, u64, SearchSession)> = None;

        for (peer, copy) in held {
            let hit = match lock(&copy).session(id) {
                Ok(Some(hit)) => hit,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("reading the copy of {peer}: {e:#}");
                    continue;
                }
            };
            let better = match &best {
                None => true,
                Some((best_peer, _, best_activity, _)) => {
                    let is_preferred = prefer == Some(peer);
                    let best_preferred = prefer == Some(*best_peer);
                    (is_preferred && !best_preferred)
                        || (is_preferred == best_preferred && hit.1 > *best_activity)
                }
            };

            if better {
                best = Some((peer, hit.0, hit.1, hit.2));
            }
        }

        best
    }

    pub(crate) fn copy(&self, machine: EndpointId) -> anyhow::Result<Arc<Mutex<IndexCopy>>> {
        if let Some(copy) = self.held().get(&machine) {
            return Ok(copy.clone());
        }

        fs::create_dir_all(&self.dir)?;
        let copy = Arc::new(Mutex::new(IndexCopy::open(&self.path(machine))?));
        Ok(self.held().entry(machine).or_insert(copy).clone())
    }

    /// Drops the copies of machines no longer in the mesh.
    pub(crate) fn keep(&self, members: &[EndpointId]) {
        let gone: Vec<EndpointId> = self
            .held()
            .keys()
            .filter(|id| !members.contains(id))
            .copied()
            .collect();

        for machine in gone {
            self.held().remove(&machine);
            let _ = fs::remove_file(self.path(machine));
        }
    }

    fn path(&self, machine: EndpointId) -> PathBuf {
        self.dir.join(format!("{machine}.sqlite"))
    }

    fn held(&self) -> MutexGuard<'_, HashMap<EndpointId, Arc<Mutex<IndexCopy>>>> {
        lock(&self.held)
    }
}

fn machine_of(path: &Path) -> Option<EndpointId> {
    (path.extension()? == "sqlite").then_some(())?;
    path.file_stem()?.to_str()?.parse().ok()
}

/// One session's metadata from the kept copies, when indexed.
pub(crate) async fn find_session(
    app: &AppHandle,
    id: &SessionId,
    machine: Option<EndpointId>,
    prefer: Option<EndpointId>,
) -> Option<(EndpointId, SessionId, u64, SearchSession)> {
    let copies = app.state::<Arc<Copies>>().inner().clone();
    let id = id.clone();
    spawn_blocking(move || copies.find_session(&id, machine, prefer))
        .await
        .ok()
        .flatten()
}
