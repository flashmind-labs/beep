//! What a project's task shares with the pulls and the node.

use super::{
    list::merge_peer,
    state::{Cursor, State, read_saved},
};
use crate::files::sync::{
    disk::{self},
    gitdir::{self, Oid, Repo},
    map::{Changes, Entry, Replica, is_conflict, original_of, valid_path},
    rules::Rules,
    scan::{MAX_FILES, Stat},
    watch::Key,
};
use crate::files::transfer::place::Folder;
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::{Keep, ProjectStatus};
use iroh::{EndpointAddr, EndpointId};
use iroh_blobs::Hash;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, RwLock},
    time::Duration,
};
use tokio::{
    sync::{Notify, watch},
    task::spawn_blocking,
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;

/// The longest a scan before matching a peer may take; the pull tries again.
const MATCH_SCAN: Duration = Duration::from_secs(30);

/// What a project's task shares with the pulls and the node.
pub(in crate::files::sync) struct Shared {
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
    /// The peers yet to send what they had, or to prove out of reach. Until
    /// they have, this node cannot say it is in sync.
    unheard: Mutex<HashSet<EndpointId>>,
    /// When each peer last sent all it had, from a pull that went through.
    /// Unlike `unheard`, a pull that failed does not set it.
    caught: Mutex<HashMap<EndpointId, Instant>>,
    /// When the last round that brought the folder in line with the list
    /// began. It saw every merge made before that.
    applied: Mutex<Instant>,
    /// Raised to end the pulls, so each starts again from the peer's list.
    pull_again: watch::Sender<u64>,
}

/// What is needed to start a project.
pub(in crate::files::sync) struct Start {
    pub(in crate::files::sync) name: String,
    pub(in crate::files::sync) key: Key,
    pub(in crate::files::sync) root: Arc<Folder>,
    pub(in crate::files::sync) peers: Vec<EndpointAddr>,
    /// Where its state is kept.
    pub(in crate::files::sync) file: PathBuf,
    pub(in crate::files::sync) paused: bool,
}

impl Shared {
    /// Loads the project's state; blocks.
    pub(in crate::files::sync) fn load(start: Start) -> Result<Arc<Self>> {
        let saved = read_saved(&start.file);
        let rules = Rules::load(start.root.path())?;
        let replica = saved.map.replica();
        let seq = saved.map.seq();
        let unheard = start.peers.iter().map(|addr| addr.id).collect();
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
            unheard: Mutex::new(unheard),
            caught: Mutex::default(),
            applied: Mutex::new(Instant::now()),
            pull_again: watch::Sender::new(0),
        }))
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        crate::lock(&self.state)
    }

    pub(in crate::files::sync) fn name(&self) -> &str {
        &self.name
    }

    pub(in crate::files::sync) fn root(&self) -> &Path {
        self.root.path()
    }

    pub(in crate::files::sync) fn key(&self) -> Key {
        self.key
    }

    /// Where its state is kept.
    pub(in crate::files::sync) fn file(&self) -> &Path {
        &self.file
    }

    pub(in crate::files::sync) fn replica(&self) -> Replica {
        self.replica
    }

    pub(in crate::files::sync) fn subscribe(&self) -> watch::Receiver<u64> {
        self.seq.subscribe()
    }

    pub(in crate::files::sync) fn status(&self) -> ProjectStatus {
        self.status.borrow().clone()
    }

    pub(in crate::files::sync) fn watch_status(&self) -> watch::Receiver<ProjectStatus> {
        self.status.subscribe()
    }

    pub(in crate::files::sync) fn has_peer(&self, peer: &EndpointId) -> bool {
        self.peers.borrow().iter().any(|addr| addr.id == *peer)
    }

    pub(in crate::files::sync) fn watch_peers(&self) -> watch::Receiver<Vec<EndpointAddr>> {
        self.peers.subscribe()
    }

    pub(in crate::files::sync) fn peer_ids(&self) -> Vec<EndpointId> {
        self.peers.borrow().iter().map(|addr| addr.id).collect()
    }

    pub(in crate::files::sync) fn set_peers(&self, peers: Vec<EndpointAddr>) {
        self.peers.send_if_modified(|now| {
            let changed = *now != peers;
            *now = peers;
            changed
        });
    }

    /// Whether this node holds `hash` of the project, to serve it.
    pub(in crate::files::sync) fn has(&self, hash: &Hash) -> bool {
        self.lock().have.contains_key(hash.as_bytes())
    }

    /// The last change after `since`, if there is one.
    pub(in crate::files::sync) fn last_change_after(&self, since: u64) -> Option<u64> {
        Some(self.lock().saved.map.last_change()).filter(|&last| last > since)
    }

    /// The pulls start over: every peer has to be heard again.
    pub(super) fn listen_again(&self) {
        *crate::lock(&self.unheard) = self.peer_ids().into_iter().collect();
    }

    /// `peer` sent what it had, or could not be asked.
    pub(in crate::files::sync) fn heard(&self, peer: &EndpointId) {
        if crate::lock(&self.unheard).remove(peer) {
            // Publishes the status again.
            self.merged.notify_one();
        }
    }

    /// `peer` sent all it had, and a pull that went through brought it.
    pub(in crate::files::sync) fn caught_up(&self, peer: &EndpointId) {
        crate::lock(&self.caught).insert(*peer, Instant::now());
        // Publishes the status again, for whoever waits on it.
        self.merged.notify_one();
    }

    /// A round that began at `began` is done.
    pub(super) fn applied_from(&self, began: Instant) {
        *crate::lock(&self.applied) = began;
        // Whoever waits for a round after a pull looks again.
        self.status.send_modify(|_| {});
    }

    /// Whether a pull that went through brought all `peer` had after
    /// `since`, and a round of bringing the folder in line has come after
    /// it. A peer that is out of reach never does. What the round could
    /// not do yet is in the status.
    pub(in crate::files::sync) fn caught_up_since(
        &self,
        peer: &EndpointId,
        since: Instant,
    ) -> bool {
        let caught = crate::lock(&self.caught).get(peer).copied();
        caught.is_some_and(|at| at >= since && *crate::lock(&self.applied) >= at)
    }

    /// Ends the pulls; each starts again and says what its peer has.
    pub(in crate::files::sync) fn pull_again(&self) {
        self.pull_again.send_modify(|count| *count += 1);
    }

    pub(in crate::files::sync) fn watch_pull_again(&self) -> watch::Receiver<u64> {
        self.pull_again.subscribe()
    }

    pub(super) fn heard_all(&self) -> bool {
        crate::lock(&self.unheard).is_empty()
    }

    pub(in crate::files::sync) fn changes(&self, since: u64, limit: usize) -> Changes {
        self.lock().saved.map.since(since, limit)
    }

    pub(in crate::files::sync) async fn stopped(&self) {
        self.stop.cancelled().await;
    }

    pub(in crate::files::sync) fn stop(&self) {
        self.stop.cancel();
    }

    pub(in crate::files::sync) fn cursor(&self, peer: &EndpointId) -> Cursor {
        self.lock()
            .saved
            .cursors
            .get(&peer.to_string())
            .copied()
            .unwrap_or_default()
    }

    /// The commit this checkout is at, if `.git` syncs and has one.
    pub(in crate::files::sync) async fn head(&self) -> Result<Option<Oid>> {
        if !crate::read_lock(&self.rules).syncs_git() {
            return Ok(None);
        }
        let root = self.root().to_owned();
        let head = spawn_blocking(move || Repo::open(&root).and_then(|repo| repo.head())).await?;
        Ok(head)
    }

    /// Marks `peer` as merged with from now on, after a scan. Commits play no
    /// part: as with Syncthing, the newer version of a file wins and the other
    /// becomes a conflict copy, and a branch follows its later commit.
    pub(in crate::files::sync) async fn first_contact(&self, peer: &EndpointId) -> Result<()> {
        if self.is_matched(peer) {
            return Ok(());
        }
        self.scan_before_matching().await?;
        let mut state = self.lock();
        state.saved.matched.insert(peer.to_string());
        state.unsaved = true;
        drop(state);
        self.merged.notify_one();
        Ok(())
    }

    /// Whether this checkout has merged with `peer` before.
    pub(in crate::files::sync) fn is_matched(&self, peer: &EndpointId) -> bool {
        self.lock().saved.matched.contains(&peer.to_string())
    }

    /// Brings the list in line with the folder before it is compared with
    /// a peer's that never matched it, or sent to one. A list from before the
    /// user's last edits would count those as older than they are.
    pub(in crate::files::sync) async fn scan_before_matching(&self) -> Result<()> {
        if self.is_paused() {
            return Ok(());
        }
        self.rescan(MATCH_SCAN).await?;
        Ok(())
    }

    /// Merges what a peer sent, and where it got to.
    pub(in crate::files::sync) fn merge_from(
        &self,
        peer: &EndpointId,
        entries: &[Entry],
        cursor: Cursor,
    ) {
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

    pub(in crate::files::sync) fn is_paused(&self) -> bool {
        *self.paused.borrow()
    }

    /// Pauses the project or resumes it; the task follows.
    pub(in crate::files::sync) fn set_paused(&self, paused: bool) {
        self.paused.send_if_modified(|now| {
            let changed = *now != paused;
            *now = paused;
            changed
        });
    }

    /// Scans the folder now and brings it in line with what the peers
    /// sent, then says how the project stands. Waits at most `limit`.
    pub(in crate::files::sync) async fn rescan(&self, limit: Duration) -> Result<ProjectStatus> {
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
    pub(in crate::files::sync) async fn resolve(&self, path: &str, keep: Keep) -> Result<()> {
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
        if gitdir::in_git(path) {
            return self.resolve_ref(path, &original, keep).await;
        }
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

impl Shared {
    /// Settles the ref that kept the losing side of two commits to
    /// `original`: drops it, or moves `original` to it. The working tree
    /// stays as it is, and `git status` says how it differs.
    async fn resolve_ref(&self, path: &str, original: &str, keep: Keep) -> Result<()> {
        let (copy, beside) = {
            let state = self.lock();
            let hash = |p: &str| {
                state
                    .saved
                    .seen
                    .get(p)
                    .map(|seen| Oid::from_bytes(seen.hash))
            };
            (hash(path), hash(original))
        };
        let copy = copy.with_context(|| format!("{path} is not on this machine yet"))?;
        let root = self.root().to_owned();
        let (path, original) = (path.to_owned(), original.to_owned());
        let moved = spawn_blocking(move || -> Result<bool> {
            let repo = Repo::open(&root).context("the repository is not here")?;
            let moved = keep == Keep::Copy;
            if moved {
                repo.set_ref(gitdir::ref_name(&original), copy, beside)?;
            }
            repo.delete_ref(gitdir::ref_name(&path), copy)?;
            Ok(moved)
        })
        .await??;
        if moved {
            let mut state = self.lock();
            state.saved.index_stale = true;
            state.unsaved = true;
        }
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
