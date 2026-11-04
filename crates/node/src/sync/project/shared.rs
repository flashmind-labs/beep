//! What a project's task shares with the pulls and the node.

use super::{
    list::merge_peer,
    state::{Cursor, State, read_saved},
};
use crate::sync::{
    disk::{self},
    map::{Changes, Entry, Replica, is_conflict, original_of, valid_path},
    rules::Rules,
    scan::{MAX_FILES, Stat},
    watch::Key,
};
use crate::transfer::place::Folder;
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::{Keep, ProjectStatus};
use iroh::{EndpointAddr, EndpointId};
use iroh_blobs::Hash;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock},
    time::Duration,
};
use tokio::{
    sync::{Notify, watch},
    task::spawn_blocking,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

/// What a project's task shares with the pulls and the node.
pub(in crate::sync) struct Shared {
    pub(super) name: String,
    pub(super) key: Key,
    pub(super) root: Arc<Folder>,
    pub(super) file: PathBuf,
    pub(super) replica: Replica,
    pub(super) state: Mutex<State>,
    pub(super) rules: Arc<RwLock<Rules>>,
    /// The list's last change.
    pub(super) seq: watch::Sender<u64>,
    pub(super) status: watch::Sender<ProjectStatus>,
    pub(super) peers: watch::Sender<Vec<EndpointAddr>>,
    /// Wakes the task when a pull merged something.
    pub(super) merged: Notify,
    /// Wakes the task to scan when this node changed the folder itself.
    pub(super) touched: Notify,
    /// Wakes the task to scan at once, for `rescan`.
    pub(super) forced: Notify,
    /// How many scans `forced` asked for are done.
    pub(super) rescans: watch::Sender<u64>,
    /// Paused by the server: no scans, fetches or pulls.
    pub(super) paused: watch::Sender<bool>,
    pub(super) stop: CancellationToken,
}

/// What is needed to start a project.
pub(in crate::sync) struct Start {
    pub(in crate::sync) name: String,
    pub(in crate::sync) key: Key,
    pub(in crate::sync) root: Arc<Folder>,
    pub(in crate::sync) peers: Vec<EndpointAddr>,
    /// Where its state is kept.
    pub(in crate::sync) file: PathBuf,
    pub(in crate::sync) paused: bool,
}

impl Shared {
    /// Loads the project's state; blocks.
    pub(in crate::sync) fn load(start: Start) -> Result<Arc<Self>> {
        let saved = read_saved(&start.file);
        let rules = Rules::load(start.root.path())?;
        let replica = saved.map.replica();
        let seq = saved.map.seq();
        let status = ProjectStatus {
            name: start.name.clone(),
            folder: start.root.path().to_string_lossy().into_owned(),
            ..ProjectStatus::default()
        };
        Ok(Arc::new(Self {
            name: start.name,
            key: start.key,
            root: start.root,
            file: start.file,
            replica,
            state: Mutex::new(State::new(saved)),
            rules: Arc::new(RwLock::new(rules)),
            seq: watch::Sender::new(seq),
            status: watch::Sender::new(status),
            peers: watch::Sender::new(start.peers),
            merged: Notify::new(),
            touched: Notify::new(),
            forced: Notify::new(),
            rescans: watch::Sender::new(0),
            paused: watch::Sender::new(start.paused),
            stop: CancellationToken::new(),
        }))
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(in crate::sync) fn name(&self) -> &str {
        &self.name
    }

    pub(in crate::sync) fn root(&self) -> &Path {
        self.root.path()
    }

    pub(in crate::sync) fn key(&self) -> Key {
        self.key
    }

    /// Where its state is kept.
    pub(in crate::sync) fn file(&self) -> &Path {
        &self.file
    }

    pub(in crate::sync) fn replica(&self) -> Replica {
        self.replica
    }

    pub(in crate::sync) fn subscribe(&self) -> watch::Receiver<u64> {
        self.seq.subscribe()
    }

    pub(in crate::sync) fn status(&self) -> ProjectStatus {
        self.status.borrow().clone()
    }

    pub(in crate::sync) fn has_peer(&self, peer: &EndpointId) -> bool {
        self.peers.borrow().iter().any(|addr| addr.id == *peer)
    }

    pub(in crate::sync) fn watch_peers(&self) -> watch::Receiver<Vec<EndpointAddr>> {
        self.peers.subscribe()
    }

    pub(in crate::sync) fn peer_ids(&self) -> Vec<EndpointId> {
        self.peers.borrow().iter().map(|addr| addr.id).collect()
    }

    pub(in crate::sync) fn set_peers(&self, peers: Vec<EndpointAddr>) {
        self.peers.send_if_modified(|now| {
            let changed = *now != peers;
            *now = peers;
            changed
        });
    }

    /// Whether this node holds `hash` of the project, to serve it.
    pub(in crate::sync) fn has(&self, hash: &Hash) -> bool {
        self.lock().have.contains_key(hash.as_bytes())
    }

    pub(in crate::sync) fn changes(&self, since: u64, limit: usize) -> Changes {
        self.lock().saved.map.since(since, limit)
    }

    pub(in crate::sync) async fn stopped(&self) {
        self.stop.cancelled().await;
    }

    pub(in crate::sync) fn stop(&self) {
        self.stop.cancel();
    }

    pub(in crate::sync) fn cursor(&self, peer: &EndpointId) -> Cursor {
        self.lock()
            .saved
            .cursors
            .get(&peer.to_string())
            .copied()
            .unwrap_or_default()
    }

    /// Merges what a peer sent, and where it got to.
    pub(in crate::sync) fn merge_from(&self, peer: &EndpointId, entries: &[Entry], cursor: Cursor) {
        let mut state = self.lock();
        let merged = merge_peer(&mut state.saved.map, entries);
        if merged.refused > 0 {
            log::warn!(
                "{}: left out {} entries from {peer}: bad paths, or past {MAX_FILES} files",
                self.name,
                merged.refused
            );
        }
        let changed = merged.changed;
        state.saved.cursors.insert(peer.to_string(), cursor);
        state.unsaved = true;
        let seq = state.saved.map.seq();
        drop(state);
        if changed {
            self.seq.send_replace(seq);
        }
        self.merged.notify_one();
    }

    pub(super) fn publish(&self, update: impl FnOnce(&mut ProjectStatus)) {
        self.status.send_modify(update);
    }

    pub(in crate::sync) fn is_paused(&self) -> bool {
        *self.paused.borrow()
    }

    /// Pauses the project or resumes it; the task follows.
    pub(in crate::sync) fn set_paused(&self, paused: bool) {
        self.paused.send_if_modified(|now| {
            let changed = *now != paused;
            *now = paused;
            changed
        });
    }

    /// Scans the folder now and brings it in line with what the peers
    /// sent, then says how the project stands. Waits at most `limit`.
    pub(in crate::sync) async fn rescan(&self, limit: Duration) -> Result<ProjectStatus> {
        if self.is_paused() {
            bail!("sync of {} is paused; resume it to sync", self.name);
        }
        let mut done = self.rescans.subscribe();
        let before = *done.borrow_and_update();
        self.forced.notify_one();
        timeout(limit, done.wait_for(|count| *count > before))
            .await
            .with_context(|| format!("{} did not finish scanning in time", self.name))?
            .context("the project stopped")?;
        Ok(self.status())
    }

    /// Settles the conflict copy at `path`: deletes it, or puts it in
    /// place of the file it stands beside. The folder changes on this
    /// machine only; the scan that follows syncs it.
    pub(in crate::sync) async fn resolve(&self, path: &str, keep: Keep) -> Result<()> {
        if self.is_paused() {
            bail!(
                "sync of {} is paused; resume it to settle conflicts",
                self.name
            );
        }
        if !valid_path(path) || !is_conflict(path) {
            bail!("{path} is not a conflict copy");
        }
        let original =
            original_of(path).with_context(|| format!("{path} is not a conflict copy"))?;
        let (copy, beside) = {
            let state = self.lock();
            let stat = |p: &str| state.saved.seen.get(p).map(|seen| seen.stat);
            (stat(path), stat(&original))
        };
        let copy = copy.with_context(|| format!("{path} is not on this machine yet"))?;
        let (root, path) = (self.root.clone(), path.to_owned());
        let settle_it = Settle { copy, beside, keep };
        spawn_blocking(move || settle(&root, &path, &original, settle_it)).await??;
        self.touched.notify_one();
        Ok(())
    }
}

/// What the scan last saw of a conflict copy and the file beside it, and
/// which to keep.
struct Settle {
    copy: Stat,
    beside: Option<Stat>,
    keep: Keep,
}

/// Settles conflict copy `path` of `original` on disk, if neither changed
/// since the last scan; blocks.
fn settle(root: &Folder, path: &str, original: &str, settle: Settle) -> Result<()> {
    const CHANGED: &str = "changed on this machine since it was scanned; try again in a moment";
    match settle.keep {
        Keep::Original => {
            if !disk::remove(root, path, &settle.copy)? {
                bail!("{path} {CHANGED}");
            }
        }
        Keep::Copy => {
            if let Some(beside) = settle.beside
                && !disk::remove(root, original, &beside)?
            {
                bail!("{original} {CHANGED}");
            }
            if disk::rename(root, path, original, &settle.copy)?.is_none() {
                bail!("{path} {CHANGED}");
            }
        }
    }
    Ok(())
}
