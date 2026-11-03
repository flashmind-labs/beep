//! One synced project on this node: its list, what its folder held at the
//! last scan, and the task that keeps the two in step.
//!
//! The task scans the folder when the watcher says it changed (or on a
//! timer when it cannot be watched), records the edits it finds in the
//! list, and then applies the list to the folder: fetching files other
//! nodes changed, removing deleted ones, and moving the losing side of a
//! conflict to its copy. Pulls from peers merge into the list from their
//! own tasks and wake this one.

use super::{
    Parts,
    disk::{self, Found as OnDisk, same},
    map::{
        Changes, Entry, FileMeta, Map, Order, Replica, Version, is_conflict, original_of,
        valid_path,
    },
    peer,
    progress::{self, Meter},
    root::{self, Checked, RootId},
    rules::{MARKER, Rules},
    scan::{self, Found, MAX_BYTES, MAX_FILES, Stat},
    watch::{Key, Mode, RESCAN, Watcher},
};
use crate::transfer::place::{Folder, Partial, Reserved, create_file_at, space_at};
use anyhow::{Context, Result, anyhow, bail};
use bao_tree::io::BaoContentItem;
use beep_proto::node::sync::{Keep, MAX_WAITING, ProjectStatus, State as SyncState};
use futures::StreamExt as _;
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use iroh_blobs::{
    BlobFormat, Hash, HashAndFormat,
    api::{Store, TempTag, remote::GetProgressItem},
    get::request::{GetBlobItem, get_blob},
    hashseq::HashSeq,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ffi::OsString,
    fs,
    io::{SeekFrom, Write as _},
    mem,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock},
    time::Duration,
};
use tokio::{
    fs::File as AsyncFile,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::{Notify, mpsc, watch},
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, sleep_until, timeout},
};
use tokio_util::sync::CancellationToken;

/// How long the folder must be quiet after a change before it is scanned.
const SETTLE: Duration = Duration::from_millis(300);
/// How long the folder must be quiet before files a scan found missing are
/// sent as deletes. Deleting the whole folder (`rm -rf`, Finder) removes
/// its files first, and the folder itself last; the watcher reports each
/// removal as it happens, so the window only has to outlast a pause in
/// that stream (a slow disk, a descheduled `rm`), not the whole deletion.
/// Five seconds is well past such pauses and still sends an ordinary
/// delete within seconds.
const DELETE_SETTLE: Duration = Duration::from_secs(5);
/// How often a watched folder is scanned anyway, for changes a watcher
/// missed.
const FULL_RESCAN: Duration = Duration::from_secs(10 * 60);
/// How soon files that could not be fetched are tried again.
const RETRY: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a fetch may wait for its next data before it gives up. A
/// connection can stay open on a path that no longer carries data (a NAT
/// mapping gone, with the relay keeping the connection alive); without a
/// limit such a fetch waits forever. The next round fetches again over a
/// new connection and keeps what came.
const STALL: Duration = Duration::from_secs(30);
/// What every disk keeps free beyond a fetch.
const SPACE_MARGIN: u64 = 64 << 20;
const CHUNK: usize = 1 << 20;
/// How often a changing state is saved; it is saved on stop as well.
const SAVE_EVERY: Duration = Duration::from_secs(2);
/// How often progress is published while files are brought in line.
const SHOW_EVERY: Duration = Duration::from_millis(500);

/// The last change this node has from a peer's replica.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Cursor {
    pub(super) replica: Replica,
    pub(super) seq: u64,
}

/// A file as the last scan or write left it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Seen {
    stat: Stat,
    #[serde(with = "serde_bytes")]
    hash: [u8; 32],
    /// The version of the list entry its content is.
    version: Version,
}

/// What a project keeps on disk between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Saved {
    map: Map,
    seen: BTreeMap<String, Seen>,
    /// By peer key.
    cursors: BTreeMap<String, Cursor>,
    /// The folder it belongs to.
    #[serde(default)]
    root: Option<RootId>,
}

struct State {
    saved: Saved,
    /// How many seen files hold each hash: what this node can serve.
    have: HashMap<[u8; 32], u32>,
    unsaved: bool,
}

impl State {
    fn new(saved: Saved) -> Self {
        let mut have = HashMap::new();
        for seen in saved.seen.values() {
            *have.entry(seen.hash).or_default() += 1;
        }
        Self {
            saved,
            have,
            unsaved: false,
        }
    }

    /// A copy of what to save, once after each change.
    fn snapshot(&mut self) -> Option<Saved> {
        if !self.unsaved {
            return None;
        }
        self.unsaved = false;
        Some(self.saved.clone())
    }

    fn set_seen(&mut self, path: &str, seen: Seen) {
        *self.have.entry(seen.hash).or_default() += 1;
        if let Some(old) = self.saved.seen.insert(path.to_owned(), seen) {
            self.forget_hash(&old.hash);
        }
        self.unsaved = true;
    }

    fn remove_seen(&mut self, path: &str) -> Option<Seen> {
        let old = self.saved.seen.remove(path)?;
        self.forget_hash(&old.hash);
        self.unsaved = true;
        Some(old)
    }

    fn forget_hash(&mut self, hash: &[u8; 32]) {
        if let Some(count) = self.have.get_mut(hash) {
            *count -= 1;
            if *count == 0 {
                self.have.remove(hash);
            }
        }
    }
}

/// What a project's task shares with the pulls and the node.
pub(super) struct Shared {
    name: String,
    key: Key,
    root: Arc<Folder>,
    file: PathBuf,
    replica: Replica,
    state: Mutex<State>,
    rules: Arc<RwLock<Rules>>,
    /// The list's last change.
    seq: watch::Sender<u64>,
    status: watch::Sender<ProjectStatus>,
    peers: watch::Sender<Vec<EndpointAddr>>,
    /// Wakes the task when a pull merged something.
    merged: Notify,
    /// Wakes the task to scan when this node changed the folder itself.
    touched: Notify,
    /// Wakes the task to scan at once, for `rescan`.
    forced: Notify,
    /// How many scans `forced` asked for are done.
    rescans: watch::Sender<u64>,
    /// Paused by the server: no scans, fetches or pulls.
    paused: watch::Sender<bool>,
    stop: CancellationToken,
}

fn read_saved(file: &Path) -> Saved {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Saved::default(),
        Err(e) => {
            log::warn!("starting over: could not read {}: {e}", file.display());
            return Saved::default();
        }
    };
    match rmp_serde::from_slice::<Saved>(&bytes) {
        Ok(mut saved) => {
            saved.map = saved.map.loaded();
            saved
        }
        Err(e) => {
            // Kept aside, not lost, and a fresh replica starts.
            let aside = file.with_extension("unreadable");
            log::warn!(
                "starting over: {} is unreadable ({e}); kept as {}",
                file.display(),
                aside.display()
            );
            if let Err(e) = fs::rename(file, &aside) {
                log::warn!("could not keep {}: {e}", file.display());
            }
            Saved::default()
        }
    }
}

fn write_atomic(file: &Path, bytes: &[u8]) -> Result<()> {
    let temp = file.with_extension("tmp");
    let mut out = fs::File::create(&temp).with_context(|| format!("writing {}", temp.display()))?;
    out.write_all(bytes)?;
    out.sync_data()?;
    fs::rename(&temp, file).with_context(|| format!("writing {}", file.display()))
}

/// What is needed to start a project.
pub(super) struct Start {
    pub(super) name: String,
    pub(super) key: Key,
    pub(super) root: Arc<Folder>,
    pub(super) peers: Vec<EndpointAddr>,
    /// Where its state is kept.
    pub(super) file: PathBuf,
    pub(super) paused: bool,
}

impl Shared {
    /// Loads the project's state; blocks.
    pub(super) fn load(start: Start) -> Result<Arc<Self>> {
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

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn root(&self) -> &Path {
        self.root.path()
    }

    pub(super) fn key(&self) -> Key {
        self.key
    }

    /// Where its state is kept.
    pub(super) fn file(&self) -> &Path {
        &self.file
    }

    pub(super) fn replica(&self) -> Replica {
        self.replica
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<u64> {
        self.seq.subscribe()
    }

    pub(super) fn status(&self) -> ProjectStatus {
        self.status.borrow().clone()
    }

    pub(super) fn has_peer(&self, peer: &EndpointId) -> bool {
        self.peers.borrow().iter().any(|addr| addr.id == *peer)
    }

    pub(super) fn watch_peers(&self) -> watch::Receiver<Vec<EndpointAddr>> {
        self.peers.subscribe()
    }

    pub(super) fn peer_ids(&self) -> Vec<EndpointId> {
        self.peers.borrow().iter().map(|addr| addr.id).collect()
    }

    pub(super) fn set_peers(&self, peers: Vec<EndpointAddr>) {
        self.peers.send_if_modified(|now| {
            let changed = *now != peers;
            *now = peers;
            changed
        });
    }

    /// Whether this node holds `hash` of the project, to serve it.
    pub(super) fn has(&self, hash: &Hash) -> bool {
        self.lock().have.contains_key(hash.as_bytes())
    }

    pub(super) fn changes(&self, since: u64, limit: usize) -> Changes {
        self.lock().saved.map.since(since, limit)
    }

    pub(super) async fn stopped(&self) {
        self.stop.cancelled().await;
    }

    pub(super) fn stop(&self) {
        self.stop.cancel();
    }

    pub(super) fn cursor(&self, peer: &EndpointId) -> Cursor {
        self.lock()
            .saved
            .cursors
            .get(&peer.to_string())
            .copied()
            .unwrap_or_default()
    }

    /// Merges what a peer sent, and where it got to.
    pub(super) fn merge_from(&self, peer: &EndpointId, entries: &[Entry], cursor: Cursor) {
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

    fn publish(&self, update: impl FnOnce(&mut ProjectStatus)) {
        self.status.send_modify(update);
    }

    pub(super) fn is_paused(&self) -> bool {
        *self.paused.borrow()
    }

    /// Pauses the project or resumes it; the task follows.
    pub(super) fn set_paused(&self, paused: bool) {
        self.paused.send_if_modified(|now| {
            let changed = *now != paused;
            *now = paused;
            changed
        });
    }

    /// Scans the folder now and brings it in line with what the peers
    /// sent, then says how the project stands. Waits at most `limit`.
    pub(super) async fn rescan(&self, limit: Duration) -> Result<ProjectStatus> {
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
    pub(super) async fn resolve(&self, path: &str, keep: Keep) -> Result<()> {
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

/// One file to bring in line with the list.
#[derive(Debug)]
struct Work {
    path: String,
    /// The entry it is brought to.
    version: Version,
    /// `None` to delete it.
    file: Option<FileMeta>,
    /// What the last scan saw there.
    expected: Option<Stat>,
}

/// A fetched blob, kept and with its disk space held until written.
struct Fetched {
    _tag: TempTag,
    _room: Reserved,
}

/// The project's task.
struct Task {
    shared: Arc<Shared>,
    parts: Parts,
    watcher: Watcher,
    mode: Mode,
    /// The first scan checks the store still holds what it saw.
    first: bool,
    /// The hashes the store's tag for the project protects now.
    protected: Option<Hash>,
    blobs: HashMap<EndpointId, Connection>,
    pending: u64,
    error: Option<String>,
    /// The last scan failed, so nothing on disk is recorded.
    scan_failed: bool,
    saved_at: Instant,
    /// Files the last scan found missing: still seen, and not deleted in
    /// the list until `send_deletes`.
    held: BTreeSet<String>,
    /// The paths the last round could not bring, tried again later.
    waiting: Vec<String>,
}

/// When held deletes are due: once the folder was quiet for
/// `DELETE_SETTLE`. Times are passed in, so it runs on any clock.
#[derive(Debug, Default)]
struct DeleteTimer {
    due: Option<Instant>,
}

impl DeleteTimer {
    /// The folder changed at `now`: held deletes wait again.
    fn changed(&mut self, now: Instant) {
        if let Some(due) = &mut self.due {
            *due = now + DELETE_SETTLE;
        }
    }

    /// A scan ended at `now`, holding deletes or not.
    fn scanned(&mut self, holds: bool, now: Instant) {
        if !holds {
            self.due = None;
        } else if self.due.is_none() {
            self.due = Some(now + DELETE_SETTLE);
        }
    }
}

/// Runs the project until it stops.
pub(super) async fn run(shared: Arc<Shared>, parts: Parts, watcher: Watcher) {
    let (wake, mut woken) = mpsc::channel(1);
    watcher.add(shared.key, shared.root(), shared.rules.clone(), wake);
    // The folder itself is watched before the first walk, so a change
    // made during it is not missed; the walk adds the rest.
    let first_mode = watcher
        .set_dirs(shared.key, vec![shared.root().to_owned()])
        .await;
    let mut task = Task::new(shared.clone(), parts, watcher.clone(), first_mode);
    let mut peers = shared.peers.subscribe();
    let mut paused = shared.paused.subscribe();
    let mut pulls = JoinSet::new();
    // Scanned before anything is pulled, so edits made while the node was
    // down merge as edits. A paused project does this once resumed.
    if !*paused.borrow_and_update() {
        task.scan().await;
        task.apply().await;
        task.save(true).await;
        start_pulls(&mut pulls, &shared, &task.parts, &peers.borrow_and_update());
    }
    let mut settle: Option<Instant> = None;
    let mut deletes = DeleteTimer::default();
    deletes.scanned(!task.held.is_empty(), Instant::now());
    let mut last_scan = Instant::now();
    loop {
        if *paused.borrow_and_update() {
            // Nothing moves: the pulls stop, and changes on disk wait for
            // the scan on resume.
            pulls = JoinSet::new();
            settle = None;
            deletes = DeleteTimer::default();
            task.publish_status(None);
            let unsaved = shared.lock().unsaved;
            tokio::select! {
                () = shared.stop.cancelled() => break,
                changed = paused.changed() => if changed.is_err() {
                    break;
                },
                () = sleep_until(task.saved_at + SAVE_EVERY), if unsaved => {}
            }
            task.save(false).await;
            if !*paused.borrow() {
                while woken.try_recv().is_ok() {}
                task.scan().await;
                last_scan = Instant::now();
                task.apply().await;
                deletes.scanned(!task.held.is_empty(), last_scan);
                start_pulls(&mut pulls, &shared, &task.parts, &peers.borrow_and_update());
            }
            continue;
        }
        let every = match task.mode {
            Mode::Watching => FULL_RESCAN,
            Mode::Rescan { .. } => RESCAN,
        };
        let mut apply = false;
        let mut forced = false;
        // A change waiting to be saved is saved once `SAVE_EVERY` passed.
        let unsaved = shared.lock().unsaved;
        tokio::select! {
            () = shared.stop.cancelled() => break,
            Some(()) = woken.recv() => {
                let now = Instant::now();
                settle.get_or_insert(now + SETTLE);
                deletes.changed(now);
            }
            () = shared.touched.notified() => {
                let now = Instant::now();
                settle.get_or_insert(now + SETTLE);
                deletes.changed(now);
            }
            changed = paused.changed() => if changed.is_err() {
                break;
            },
            () = sleep_until(settle.unwrap_or_else(Instant::now)), if settle.is_some() => {
                settle = None;
                task.scan().await;
                last_scan = Instant::now();
                apply = true;
            }
            () = sleep_until(deletes.due.unwrap_or_else(Instant::now)), if deletes.due.is_some() => {
                deletes.due = None;
                // Quiet long enough: scanned again, and what is still
                // missing is sent unless the folder changed meanwhile.
                task.scan().await;
                last_scan = Instant::now();
                if woken.try_recv().is_ok() {
                    settle.get_or_insert(last_scan + SETTLE);
                } else {
                    task.send_deletes().await;
                }
                apply = true;
            }
            () = sleep_until(last_scan + every) => {
                task.scan().await;
                last_scan = Instant::now();
                apply = true;
            }
            () = shared.forced.notified() => {
                settle = None;
                task.scan().await;
                last_scan = Instant::now();
                apply = true;
                forced = true;
            }
            () = shared.merged.notified() => apply = true,
            () = sleep(RETRY), if task.pending > 0 => apply = true,
            () = sleep_until(task.saved_at + SAVE_EVERY), if unsaved => {}
            changed = peers.changed() => {
                if changed.is_err() {
                    break;
                }
                pulls = JoinSet::new();
                start_pulls(&mut pulls, &shared, &task.parts, &peers.borrow_and_update());
            }
            Some(ended) = pulls.join_next() => if let Err(e) = ended {
                log::warn!("a pull of {} failed: {e}", shared.name);
            },
        }
        deletes.scanned(!task.held.is_empty(), Instant::now());
        if apply && settle.is_none() {
            task.apply().await;
        }
        task.save(false).await;
        if forced {
            shared.rescans.send_modify(|count| *count += 1);
        }
    }
    watcher.remove(shared.key).await;
    task.save(true).await;
}

fn start_pulls(
    pulls: &mut JoinSet<()>,
    shared: &Arc<Shared>,
    parts: &Parts,
    peers: &[EndpointAddr],
) {
    for peer in peers {
        pulls.spawn(peer::pull(
            shared.clone(),
            parts.endpoint.clone(),
            peer.clone(),
        ));
    }
}

impl Task {
    fn new(shared: Arc<Shared>, parts: Parts, watcher: Watcher, mode: Mode) -> Self {
        Self {
            shared,
            parts,
            watcher,
            mode,
            first: true,
            protected: None,
            blobs: HashMap::new(),
            pending: 0,
            error: None,
            scan_failed: false,
            saved_at: Instant::now(),
            held: BTreeSet::new(),
            waiting: Vec::new(),
        }
    }

    fn fail(&mut self, what: &str, e: &anyhow::Error) {
        let message = format!("{what}: {e:#}");
        log::warn!("{}: {message}", self.shared.name);
        self.error = Some(message);
    }

    /// Looks for edits on disk and records them in the list.
    async fn scan(&mut self) {
        self.shared.publish(|s| s.state = SyncState::Scanning);
        let result = self.try_scan().await;
        self.scan_failed = result.is_err();
        if self.scan_failed {
            // Not trusted until a scan succeeds again.
            self.held.clear();
        }
        match result {
            Ok(()) => self.error = None,
            Err(e) => self.fail("scanning failed", &e),
        }
        self.publish_status(None);
    }

    async fn try_scan(&mut self) -> Result<()> {
        let checked = self.check_root().await?;
        let root = self.shared.root().to_owned();
        let rules = {
            let root = root.clone();
            spawn_blocking(move || Rules::load(&root)).await??
        };
        *self
            .shared
            .rules
            .write()
            .unwrap_or_else(PoisonError::into_inner) = rules;
        let walked = {
            let (root, rules) = (root.clone(), self.shared.rules.clone());
            spawn_blocking(move || {
                let rules = rules.read().unwrap_or_else(PoisonError::into_inner);
                scan::walk(&root, &rules, 0)
            })
            .await??
        };
        self.mode = self
            .watcher
            .set_dirs(self.shared.key, walked.dirs.clone())
            .await;
        let prior: Vec<Option<Seen>> = {
            let state = self.shared.lock();
            walked
                .files
                .iter()
                .map(|found| state.saved.seen.get(&found.path).cloned())
                .collect()
        };
        // Hashing reads the files; the list is locked only to record.
        let mut tags = Vec::new();
        let mut hashed = Vec::new();
        for (found, prior) in walked.files.iter().zip(prior) {
            if let Some(prior) = &prior
                && same(&found.stat, &prior.stat)
                && (!self.first
                    || self
                        .parts
                        .store
                        .blobs()
                        .has(Hash::from_bytes(prior.hash))
                        .await?)
            {
                continue;
            }
            if self.shared.stop.is_cancelled() {
                bail!("stopped");
            }
            match scan::import(&self.parts.store, found).await {
                Ok(tag) => {
                    hashed.push((found, *tag.hash().as_bytes(), prior));
                    tags.push(tag);
                }
                // Changed while it was hashed: the next scan sees it again.
                Err(e) => log::debug!("{}: {e:#}", self.shared.name),
            }
        }
        let first = self.first;
        let kept: HashSet<&str> = walked.files.iter().map(|f| f.path.as_str()).collect();
        let (gone, seen_count): (Vec<String>, usize) = {
            let state = self.shared.lock();
            let gone = state
                .saved
                .seen
                .keys()
                .filter(|path| !kept.contains(path.as_str()))
                .cloned()
                .collect();
            (gone, state.saved.seen.len())
        };
        // A file the rules left out since, or git took over, is not
        // deleted elsewhere; only one missing from disk is.
        let deleted = {
            let root = self.shared.root.clone();
            spawn_blocking(move || {
                gone.into_iter()
                    .map(|path| {
                        let missing = matches!(disk::look(&root, &path), Ok(OnDisk::Missing));
                        (path, missing)
                    })
                    .collect::<Vec<_>>()
            })
            .await?
        };
        if let Checked::Unmarked(saved) = checked {
            // Files gone with the marker: the folder is being deleted.
            if deleted.iter().any(|(_, missing)| *missing) {
                bail!(
                    "{} lost its {MARKER} along with files; nothing is synced until it \
                     is back",
                    root.display()
                );
            }
            let folder = self.shared.root.clone();
            spawn_blocking(move || root::restore(&folder, &saved)).await??;
        }
        let baseline = has_baseline(first, seen_count, &deleted);
        if !baseline {
            log::warn!(
                "{}: none of the {seen_count} files synced before are in {}; they are \
                 fetched again, not deleted elsewhere",
                self.shared.name,
                self.shared.root().display()
            );
        }
        let mut held = BTreeSet::new();
        let (before, seq) = {
            let mut state = self.shared.lock();
            let before = state.saved.map.seq();
            for (found, hash, prior) in hashed {
                record(&mut state, found, hash, prior);
            }
            for (path, missing) in deleted {
                // Kept as seen, and sent by `send_deletes` once the
                // folder settled and is still the one synced.
                if missing && baseline {
                    held.insert(path);
                } else {
                    state.remove_seen(&path);
                }
            }
            (before, state.saved.map.seq())
        };
        self.held = held;
        self.first = false;
        if seq != before {
            self.shared.seq.send_replace(seq);
        }
        self.protect().await?;
        drop(tags);
        Ok(())
    }

    /// Records the deletes the last scan held, once the folder is still
    /// the one synced before, marker and all. Called after the folder was
    /// quiet for `DELETE_SETTLE` and scanned again; a folder that went
    /// away or changed meanwhile sends none and stops syncing.
    async fn send_deletes(&mut self) {
        let held = mem::take(&mut self.held);
        if held.is_empty() {
            return;
        }
        if let Err(e) = self.try_send_deletes(held).await {
            self.scan_failed = true;
            self.fail("not syncing", &e);
            self.publish_status(None);
        }
    }

    async fn try_send_deletes(&mut self, held: BTreeSet<String>) -> Result<()> {
        let (saved, seen_any) = self.saved_root();
        let folder = self.shared.root.clone();
        let checked = {
            let saved = saved.clone();
            spawn_blocking(move || root::check(&folder, saved.as_ref(), seen_any)).await??
        };
        match (checked, saved) {
            (Checked::Same(id), Some(saved)) if id == saved => {}
            _ => bail!(
                "{} is not the folder synced before, or lost its {MARKER}; no file is \
                 deleted elsewhere",
                self.shared.root().display()
            ),
        }
        let missing = {
            let root = self.shared.root.clone();
            spawn_blocking(move || {
                held.into_iter()
                    .filter(|path| matches!(disk::look(&root, path), Ok(OnDisk::Missing)))
                    .collect::<Vec<_>>()
            })
            .await?
        };
        let (before, seq) = {
            let mut state = self.shared.lock();
            let before = state.saved.map.seq();
            for path in missing {
                if let Some(old) = state.remove_seen(&path) {
                    state.saved.map.edit(&path, &old.version, None);
                }
            }
            (before, state.saved.map.seq())
        };
        if seq != before {
            self.shared.seq.send_replace(seq);
        }
        self.protect().await
    }

    /// The folder the state belongs to, and whether any file was seen.
    fn saved_root(&self) -> (Option<RootId>, bool) {
        let state = self.shared.lock();
        (state.saved.root.clone(), !state.saved.seen.is_empty())
    }

    /// Checks the folder is the one synced before; until it is, nothing
    /// is scanned or written. One that lost its marker is left to the
    /// scan, which writes it again only if no file is missing.
    async fn check_root(&self) -> Result<Checked> {
        let (saved, seen_any) = self.saved_root();
        let folder = self.shared.root.clone();
        let checked =
            spawn_blocking(move || root::check(&folder, saved.as_ref(), seen_any)).await??;
        if let Checked::Same(id) = &checked {
            let mut state = self.shared.lock();
            if state.saved.root.as_ref() != Some(id) {
                state.saved.root = Some(id.clone());
                state.unsaved = true;
            }
        }
        Ok(checked)
    }

    /// Brings the folder in line with the list.
    async fn apply(&mut self) {
        // A merge may set this off long after the last scan; the folder
        // may have been swapped or unmounted since.
        let checked = match self.check_root().await {
            // A scan writes the marker again if the folder is whole.
            Ok(Checked::Unmarked(_)) => {
                self.scan().await;
                self.check_root().await
            }
            checked => checked,
        };
        let error = match checked {
            Ok(Checked::Same(_)) => None,
            Ok(Checked::Unmarked(_)) => Some(anyhow!(
                "{} lost its {MARKER}",
                self.shared.root().display()
            )),
            Err(e) => Some(e),
        };
        if let Some(e) = error {
            self.scan_failed = true;
            self.fail("not syncing", &e);
            self.publish_status(None);
            return;
        }
        let work = {
            let state = self.shared.lock();
            let rules = self
                .shared
                .rules
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            plan(&state, &rules)
        };
        if work.is_empty() {
            self.pending = 0;
            self.waiting.clear();
            self.publish_status(None);
            return;
        }
        self.publish_status(Some(SyncState::Syncing));
        let mut partial = None;
        let mut failed = 0;
        let total = work.len();
        let paths: Vec<String> = work.iter().map(|item| item.path.clone()).collect();
        let bytes = work
            .iter()
            .filter_map(|item| item.file.as_ref())
            .map(|file| file.size)
            .sum();
        let mut meter = Meter::new(total as u64, bytes, Instant::now());
        let mut lost: Vec<String> = Vec::new();
        self.show(&meter, progress::waiting(&lost, &paths));
        let mut shown = Instant::now();
        for (at, item) in work.into_iter().enumerate() {
            if self.shared.stop.is_cancelled() || self.shared.is_paused() {
                break;
            }
            let size = item.file.as_ref().map_or(0, |file| file.size);
            let path = item.path.clone();
            match self.bring(item, &mut partial).await {
                Ok(()) => meter.done(size, Instant::now()),
                Err(e) => {
                    failed += 1;
                    meter.failed(size);
                    if lost.len() < MAX_WAITING {
                        lost.push(path);
                    }
                    self.fail("syncing a file failed", &e);
                }
            }
            if shown.elapsed() >= SHOW_EVERY {
                shown = Instant::now();
                let ahead = paths.get(at + 1..).unwrap_or_default();
                self.show(&meter, progress::waiting(&lost, ahead));
            }
        }
        self.waiting = lost;
        if let Some(partial) = partial {
            let finished = spawn_blocking(move || partial.finish()).await;
            if let Ok(Err(e)) | Err(e) = finished.map_err(anyhow::Error::from) {
                log::warn!("a partial folder stays until the next start: {e:#}");
            }
        }
        if failed == 0 {
            self.error = None;
        }
        log::debug!("{}: {total} changes, {failed} failed", self.shared.name);
        self.pending = failed;
        if let Err(e) = self.protect().await {
            self.fail("keeping the files in the store failed", &e);
        }
        self.publish_status(None);
    }

    /// Brings one path in line, unless the list or the disk moved on since
    /// it was planned.
    async fn bring(&mut self, work: Work, partial: &mut Option<Partial>) -> Result<()> {
        let Some(file) = work.file.clone() else {
            return self.delete(&work).await;
        };
        // What the folder holds elsewhere may make a fetch unneeded: the
        // losing side of a conflict, here at `path`, moves to its copy.
        if let Some(expected) = work.expected
            && let Some(keep_as) = self.moves_to(&work.path)
        {
            self.keep_as(&work.path, &keep_as, expected).await?;
            let work = Work {
                expected: None,
                ..work
            };
            return Box::pin(self.bring(work, partial)).await;
        }
        if !self.still_wanted(&work) {
            return Ok(());
        }
        let hash = Hash::from_bytes(file.hash);
        let _fetched = if self.parts.store.blobs().has(hash).await? {
            None
        } else {
            Some(self.fetch(&work.path, &file).await?)
        };
        if partial.is_none() {
            let (journal, root) = (self.parts.journal.clone(), self.shared.root.clone());
            *partial = Some(spawn_blocking(move || journal.start(root)).await??);
        }
        let Some(partial) = partial.as_ref() else {
            bail!("no partial folder");
        };
        let temp = OsString::from(format!("{:x}", uuid::Uuid::new_v4().as_u64_pair().0));
        let created = {
            let (dir, temp, executable) =
                (partial.dir().try_clone()?, temp.clone(), file.executable);
            spawn_blocking(move || create_file_at(&dir, &temp, executable)).await??
        };
        let mut out = AsyncFile::from_std(created);
        let (store, stop) = (self.parts.store.clone(), self.shared.stop.clone());
        let path = work.path.clone();
        copy_out(
            &store,
            hash,
            &mut out,
            &stop,
            async |out: &mut AsyncFile| {
                log::info!(
                    "{}: the copy of {path} on this machine changed or cannot be read; \
                 fetching it again",
                    self.shared.name
                );
                self.copy_from_peers(hash, out).await
            },
        )
        .await?;
        out.flush().await?;
        let out = out.into_std().await;
        let stat = {
            let (root, dir) = (self.shared.root.clone(), partial.dir().try_clone()?);
            let (path, expected, mtime) = (work.path.clone(), work.expected, file.mtime_ms);
            spawn_blocking(move || {
                let written = disk::Written {
                    dir: &dir,
                    name: &temp,
                    file: out,
                    mtime_ms: mtime,
                };
                disk::place(&root, written, &path, expected.as_ref())
            })
            .await??
        };
        self.shared.lock().set_seen(
            &work.path,
            Seen {
                stat,
                hash: file.hash,
                version: work.version,
            },
        );
        Ok(())
    }

    /// Whether the list still wants `work` and the disk is still as
    /// planned.
    fn still_wanted(&self, work: &Work) -> bool {
        let state = self.shared.lock();
        let listed = state.saved.map.get(&work.path);
        let seen = state.saved.seen.get(&work.path);
        listed.is_some_and(|entry| entry.version == work.version && entry.file == work.file)
            && seen.map(|seen| seen.stat) == work.expected
    }

    /// The path the content at `path` should move to: a path the list
    /// wants that content at and that is not on disk.
    fn moves_to(&self, path: &str) -> Option<String> {
        let state = self.shared.lock();
        let rules = self
            .shared
            .rules
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        move_target(&state, &rules, path)
    }

    async fn keep_as(&mut self, path: &str, to: &str, expected: Stat) -> Result<()> {
        let moved = {
            let (root, from, to_path) = (self.shared.root.clone(), path.to_owned(), to.to_owned());
            spawn_blocking(move || disk::rename(&root, &from, &to_path, &expected)).await??
        };
        let Some(stat) = moved else {
            bail!("{path} changed on this machine; it is merged on the next scan");
        };
        let found = Found {
            path: to.to_owned(),
            abs: self.shared.root().join(to),
            stat,
        };
        // The store reads it from its new path from now on.
        let tag = scan::import(&self.parts.store, &found).await?;
        {
            let mut state = self.shared.lock();
            let version = state
                .saved
                .map
                .get(to)
                .map(|entry| entry.version.clone())
                .unwrap_or_default();
            if let Some(old) = state.remove_seen(path) {
                state.set_seen(
                    to,
                    Seen {
                        stat,
                        hash: old.hash,
                        version,
                    },
                );
            }
        }
        self.protect().await?;
        drop(tag);
        Ok(())
    }

    async fn delete(&mut self, work: &Work) -> Result<()> {
        let Some(expected) = work.expected else {
            return Ok(());
        };
        if let Some(keep_as) = self.moves_to(&work.path) {
            return self.keep_as(&work.path, &keep_as, expected).await;
        }
        if !self.still_wanted(work) {
            return Ok(());
        }
        let removed = {
            let (root, path) = (self.shared.root.clone(), work.path.clone());
            spawn_blocking(move || disk::remove(&root, &path, &expected)).await??
        };
        if removed {
            self.shared.lock().remove_seen(&work.path);
        }
        Ok(())
    }

    /// Fetches `file`'s blob from the first peer that has it.
    async fn fetch(&mut self, path: &str, file: &FileMeta) -> Result<Fetched> {
        ensure_size(file.size)?;
        let room = {
            let (store_dir, root, room) = (
                self.parts.store_dir.clone(),
                self.shared.root.clone(),
                self.parts.room.clone(),
            );
            let need = file.size;
            spawn_blocking(move || {
                room.reserve(need, space_at(&store_dir)?, root.space()?, SPACE_MARGIN)
            })
            .await??
        };
        let content = HashAndFormat::raw(Hash::from_bytes(file.hash));
        let tag = self.parts.store.tags().temp_tag(content).await?;
        let peers = self.shared.peers.borrow().clone();
        let mut last = None;
        for peer in peers {
            let connection = match self.blobs_to(&peer).await {
                Ok(connection) => connection,
                Err(e) => {
                    last = Some(e);
                    continue;
                }
            };
            let fetched = fetch_within(
                &self.parts.store,
                connection.clone(),
                content,
                STALL,
                &self.shared.stop,
            )
            .await;
            match fetched {
                Ok(()) if self.parts.store.blobs().has(content.hash).await? => {
                    return Ok(Fetched {
                        _tag: tag,
                        _room: room,
                    });
                }
                Ok(()) => last = Some(anyhow!("{} sent only part of it", peer.id)),
                Err(e) => {
                    if self.shared.stop.is_cancelled() {
                        return Err(e);
                    }
                    self.blobs.remove(&peer.id);
                    connection.close(0u32.into(), b"fetch failed");
                    last = Some(e.context(peer.id.to_string()));
                }
            }
        }
        Err(match last {
            Some(e) => e.context(format!("no machine that syncs it could send {path} now")),
            None => anyhow!("no other machine syncs {path}"),
        })
    }

    /// Writes blob `hash` into `out` from the first peer that sends all of
    /// it, verified as it comes, without the store.
    async fn copy_from_peers(&mut self, hash: Hash, out: &mut AsyncFile) -> Result<()> {
        let peers = self.shared.peers.borrow().clone();
        let mut last = anyhow!("no other machine syncs it");
        for peer in peers {
            out.set_len(0).await?;
            out.seek(SeekFrom::Start(0)).await?;
            let copied = match self.blobs_to(&peer).await {
                Ok(connection) => copy_blob(connection, hash, out, &self.shared.stop).await,
                Err(e) => Err(e),
            };
            match copied {
                Ok(copied) if copied == *hash.as_bytes() => return Ok(()),
                Ok(_) => last = anyhow!("{} sent other content", peer.id),
                Err(e) => {
                    self.blobs.remove(&peer.id);
                    last = e.context(format!("{}", peer.id));
                }
            }
        }
        Err(last.context("no machine could send it again"))
    }

    async fn blobs_to(&mut self, peer: &EndpointAddr) -> Result<Connection> {
        if let Some(connection) = self.blobs.get(&peer.id)
            && connection.close_reason().is_none()
        {
            return Ok(connection.clone());
        }
        let connection = timeout(
            CONNECT_TIMEOUT,
            self.parts.endpoint.connect(peer.clone(), iroh_blobs::ALPN),
        )
        .await
        .map_err(|_| anyhow!("could not reach {} in time", peer.id))??;
        self.blobs.insert(peer.id, connection.clone());
        Ok(connection)
    }

    /// Keeps what the folder holds in the store, for peers to fetch, under
    /// one tag for the project.
    async fn protect(&mut self) -> Result<()> {
        let mut hashes: Vec<Hash> = self
            .shared
            .lock()
            .have
            .keys()
            .map(|hash| Hash::from_bytes(*hash))
            .collect();
        hashes.sort();
        let list: HashSeq = hashes.into_iter().collect();
        let bytes = list.into_inner();
        let root = Hash::new(&bytes);
        if self.protected == Some(root) {
            return Ok(());
        }
        let tag = self
            .parts
            .store
            .blobs()
            .add_bytes_with_opts((bytes, BlobFormat::HashSeq))
            .temp_tag()
            .await?;
        self.parts
            .store
            .tags()
            .set(tag_name(self.shared.key), tag.hash_and_format())
            .await?;
        self.protected = Some(root);
        Ok(())
    }

    /// Saves the state when it changed and the last save is `SAVE_EVERY`
    /// old, or now when `now`. The state is copied under its lock and
    /// written outside it.
    async fn save(&mut self, now: bool) {
        if !now && self.saved_at.elapsed() < SAVE_EVERY {
            return;
        }
        let Some(snapshot) = self.shared.lock().snapshot() else {
            return;
        };
        self.saved_at = Instant::now();
        let file = self.shared.file.clone();
        let written = spawn_blocking(move || {
            let bytes = rmp_serde::to_vec_named(&snapshot)?;
            write_atomic(&file, &bytes)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|written| written);
        if let Err(e) = written {
            log::warn!("could not save {}: {e:#}", self.shared.name);
            // Tried again on the next pass.
            self.shared.lock().unsaved = true;
        }
    }

    /// Publishes how far the round is, and what still waits.
    fn show(&self, meter: &Meter, waiting: Vec<String>) {
        let progress = meter.progress();
        self.shared.publish(move |status| {
            status.progress = Some(progress);
            status.waiting = waiting;
        });
    }

    /// Publishes how the project stands; progress shows only while a round
    /// runs, and `show` sets it then.
    fn publish_status(&self, state: Option<SyncState>) {
        let (files, bytes, conflicts) = {
            let locked = self.shared.lock();
            let bytes = locked.saved.seen.values().map(|seen| seen.stat.size).sum();
            let conflicts = locked
                .saved
                .map
                .entries()
                .filter(|entry| entry.file.is_some() && is_conflict(&entry.path))
                .map(|entry| entry.path.clone())
                .collect();
            (locked.saved.seen.len() as u64, bytes, conflicts)
        };
        let paused = self.shared.is_paused();
        let state = state.unwrap_or(if paused {
            SyncState::Paused
        } else if self.scan_failed {
            SyncState::Failed
        } else if self.pending > 0 {
            SyncState::Syncing
        } else {
            SyncState::Idle
        });
        let (rescan_secs, hint) = match &self.mode {
            Mode::Watching => (None, None),
            Mode::Rescan { hint } => (Some(RESCAN.as_secs() as u32), Some((*hint).to_owned())),
        };
        let (pending, error) = (self.pending, self.error.clone());
        let waiting = self.waiting.clone();
        self.shared.publish(move |status| {
            status.paused = paused;
            status.progress = None;
            status.waiting = waiting;
            status.state = state;
            status.files = files;
            status.bytes = bytes;
            status.pending = pending;
            status.conflicts = conflicts;
            status.rescan_secs = rescan_secs;
            status.hint = hint;
            status.error = error;
        });
    }
}

/// Writes blob `hash` into `out` from the store, or with `refetch` when
/// the store's copy cannot be read or is not that content any more: the
/// store reads it from a file in place, which may have changed or gone
/// since. A peer sends it again, checked as it comes.
async fn copy_out(
    store: &Store,
    hash: Hash,
    out: &mut AsyncFile,
    stop: &CancellationToken,
    refetch: impl AsyncFnOnce(&mut AsyncFile) -> Result<()>,
) -> Result<()> {
    match copy_store(store, hash, out, stop).await {
        Ok(copied) if copied == *hash.as_bytes() => return Ok(()),
        Ok(_) => {}
        Err(_) if stop.is_cancelled() => bail!("stopped"),
        Err(e) => log::debug!("reading {hash} from the store: {e:#}"),
    }
    refetch(out).await
}

/// Copies blob `hash` from the store into `out`, giving the hash of what
/// was written: another hash when the file the store reads it from was
/// changed in place.
async fn copy_store(
    store: &Store,
    hash: Hash,
    out: &mut AsyncFile,
    stop: &CancellationToken,
) -> Result<[u8; 32]> {
    let mut reader = store.blobs().reader(hash);
    let mut buffer = vec![0; CHUNK];
    let mut hasher = blake3::Hasher::new();
    loop {
        if stop.is_cancelled() {
            bail!("stopped");
        }
        let read = reader
            .read(&mut buffer)
            .await
            .context("reading the store")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        out.write_all(&buffer[..read]).await?;
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Copies blob `hash` from a peer into `out`, giving the hash of what was
/// written. The blobs protocol checks every chunk against `hash`.
/// Fetches `content` into `store` over `connection`, failing when no data
/// comes for `stall`.
async fn fetch_within(
    store: &Store,
    connection: Connection,
    content: HashAndFormat,
    stall: Duration,
    stop: &CancellationToken,
) -> Result<()> {
    let items = store.remote().fetch(connection, content).stream();
    let mut items = std::pin::pin!(items);
    loop {
        let item = tokio::select! {
            item = timeout(stall, items.next()) => item
                .map_err(|_| anyhow!("no data came for {} s", stall.as_secs_f32()))?,
            () = stop.cancelled() => bail!("stopped"),
        };
        match item {
            Some(GetProgressItem::Progress(_)) => {}
            Some(GetProgressItem::Done(_)) => return Ok(()),
            Some(GetProgressItem::Error(e)) => bail!("{e}"),
            None => bail!("the fetch ended early"),
        }
    }
}

async fn copy_blob(
    connection: Connection,
    hash: Hash,
    out: &mut AsyncFile,
    stop: &CancellationToken,
) -> Result<[u8; 32]> {
    let mut items = get_blob(connection, hash);
    let mut hasher = blake3::Hasher::new();
    let mut at = 0u64;
    loop {
        let item = tokio::select! {
            item = timeout(STALL, items.next()) => item
                .map_err(|_| anyhow!("no data came for {} s", STALL.as_secs()))?,
            () = stop.cancelled() => bail!("stopped"),
        };
        match item {
            Some(GetBlobItem::Item(BaoContentItem::Leaf(leaf))) => {
                if leaf.offset != at {
                    bail!("the blob came out of order");
                }
                hasher.update(&leaf.data);
                out.write_all(&leaf.data).await?;
                at += leaf.data.len() as u64;
            }
            Some(GetBlobItem::Item(BaoContentItem::Parent(_))) => {}
            Some(GetBlobItem::Done(_)) => return Ok(*hasher.finalize().as_bytes()),
            Some(GetBlobItem::Error(e)) => return Err(e.into()),
            None => bail!("the blob ended early"),
        }
    }
}

fn ensure_size(size: u64) -> Result<()> {
    if size > MAX_BYTES {
        bail!("the file is larger than {} GiB", MAX_BYTES >> 30);
    }
    Ok(())
}

/// Whether the files a scan found missing are deletes. On the first scan
/// after a start, a folder that holds none of the files seen before was
/// emptied or replaced while sync was off: what it misses is fetched
/// again rather than deleted on every other machine.
fn has_baseline(first: bool, seen: usize, gone: &[(String, bool)]) -> bool {
    let all_missing = gone.len() == seen && gone.iter().all(|(_, missing)| *missing);
    !(first && seen > 0 && all_missing)
}

/// The name of the store tag that keeps project `key`'s files.
pub(super) fn tag_name(key: Key) -> String {
    format!("sync/{key:016x}")
}

/// Records a file the scan hashed.
fn record(state: &mut State, found: &Found, hash: [u8; 32], prior: Option<Seen>) {
    let meta = FileMeta {
        hash,
        size: found.stat.size,
        mtime_ms: found.stat.mtime_ms(),
        executable: found.stat.executable,
    };
    let version = match prior {
        // Touched, not changed.
        Some(prior) if prior.hash == hash => prior.version,
        Some(prior) => {
            // The list may hold the content seen here under a newer version:
            // a concurrent edit this content won was merged into it. The
            // edit builds on that, so it does not lose to it again.
            let base = match state.saved.map.get(&found.path) {
                Some(entry)
                    if entry.file.as_ref().is_some_and(|f| f.hash == prior.hash)
                        && entry.version.compare(&prior.version) == Order::Newer =>
                {
                    entry.version.clone()
                }
                _ => prior.version,
            };
            state.saved.map.edit(&found.path, &base, Some(meta))
        }
        None => match state.saved.map.get(&found.path) {
            // The same file arrived some other way; nothing changed.
            Some(entry) if entry.file.as_ref().is_some_and(|f| f.hash == hash) => {
                entry.version.clone()
            }
            // New here: concurrent with whatever the list has, so the two
            // are kept apart if they differ.
            _ => state
                .saved
                .map
                .edit(&found.path, &Version::default(), Some(meta)),
        },
    };
    state.set_seen(
        &found.path,
        Seen {
            stat: found.stat,
            hash,
            version,
        },
    );
}

/// Where the content at `path` should move: a path the list wants that
/// content at, that is not on disk and that the rules keep.
fn move_target(state: &State, rules: &Rules, path: &str) -> Option<String> {
    let seen = state.saved.seen.get(path)?;
    state
        .saved
        .map
        .entries()
        .filter(|entry| entry.path != path)
        .filter(|entry| entry.file.as_ref().is_some_and(|f| f.hash == seen.hash))
        .filter(|entry| !rules.skips_any(Path::new(&entry.path), false))
        .find(|entry| !state.saved.seen.contains_key(&entry.path))
        .map(|entry| entry.path.clone())
}

/// What merging a peer's entries did.
#[derive(Debug, PartialEq, Eq)]
struct Merged {
    changed: bool,
    /// Entries left out: a path no peer may name, or a new file past the
    /// file limit.
    refused: usize,
}

/// Merges a peer's `entries` into `map`.
fn merge_peer(map: &mut Map, entries: &[Entry]) -> Merged {
    let mut live = map.entries().filter(|entry| entry.file.is_some()).count();
    let mut merged = Merged {
        changed: false,
        refused: 0,
    };
    let is_live = |map: &Map, path: &str| map.get(path).is_some_and(|known| known.file.is_some());
    for entry in entries {
        let was_live = is_live(map, &entry.path);
        if !valid_path(&entry.path) || (entry.file.is_some() && !was_live && live >= MAX_FILES) {
            merged.refused += 1;
            continue;
        }
        for path in map.merge(entry) {
            merged.changed = true;
            let now_live = is_live(map, &path);
            let before = path == entry.path && was_live;
            live = live + usize::from(now_live) - usize::from(before);
        }
    }
    merged
}

/// What it takes to bring the folder in line with the list: the paths
/// whose entry is not what the folder holds, leaving out paths the rules
/// skip here. Moves to conflict copies come first.
fn plan(state: &State, rules: &Rules) -> Vec<Work> {
    let mut work: Vec<Work> = state
        .saved
        .map
        .entries()
        // A list saved before peers' paths were checked may hold others.
        .filter(|entry| valid_path(&entry.path))
        .filter(|entry| !rules.skips_any(Path::new(&entry.path), false))
        .filter_map(|entry| {
            let seen = state.saved.seen.get(&entry.path);
            match (&entry.file, seen) {
                (Some(file), Some(seen)) if seen.hash == file.hash => None,
                (None, None) => None,
                _ => Some(Work {
                    path: entry.path.clone(),
                    version: entry.version.clone(),
                    file: entry.file.clone(),
                    expected: seen.map(|seen| seen.stat),
                }),
            }
        })
        .collect();
    work.sort_by_key(|item| item.expected.is_none());
    work
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        files::Folders,
        transfer::place::{Journal, open_target},
    };
    use iroh::{Endpoint, endpoint::presets};
    use iroh_blobs::store::fs::FsStore;
    use tempfile::TempDir;

    /// A project task over a folder of files, scanned once, driven by
    /// hand: no watcher wakes it and no timer runs.
    struct Project {
        _dir: TempDir,
        base: PathBuf,
        folder: PathBuf,
        task: Task,
    }

    impl Project {
        async fn new(files: &[&str]) -> Self {
            let dir = TempDir::new().unwrap();
            let base = dir.path().canonicalize().unwrap();
            let folder = base.join("game");
            for file in files {
                let path = folder.join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, file).unwrap();
            }
            let folders = Folders::new(std::slice::from_ref(&base));
            let root = open_target(&folders, &folder.to_string_lossy()).unwrap();
            let parts = Parts {
                endpoint: Endpoint::builder(presets::Minimal).bind().await.unwrap(),
                store: FsStore::load(base.join("store")).await.unwrap(),
                store_dir: base.join("store"),
                journal: Journal::open(base.join("partial")).unwrap(),
                room: Arc::default(),
            };
            let shared = Shared::load(Start {
                name: "game".into(),
                key: 1,
                root: Arc::new(root),
                peers: Vec::new(),
                file: base.join("state"),
                paused: false,
            })
            .unwrap();
            let watcher = Watcher::with_budget(None, None);
            let mut task = Task::new(shared, parts, watcher, Mode::Watching);
            task.scan().await;
            assert_eq!(task.error, None);
            Self {
                _dir: dir,
                base,
                folder,
                task,
            }
        }

        fn path(&self, path: &str) -> PathBuf {
            self.folder.join(path)
        }

        /// The paths the list holds as deleted: what peers would delete.
        fn deleted(&self) -> Vec<String> {
            self.task
                .shared
                .lock()
                .saved
                .map
                .entries()
                .filter(|entry| entry.file.is_none())
                .map(|entry| entry.path.clone())
                .collect()
        }

        fn held(&self) -> Vec<&str> {
            self.task.held.iter().map(String::as_str).collect()
        }

        async fn stop(self) {
            self.task.parts.store.shutdown().await.ok();
            self.task.parts.endpoint.close().await;
        }
    }

    const COPY: &str = "docs/notes (conflict 1a2b3c4d).txt";

    #[tokio::test]
    async fn keeping_the_copy_puts_it_in_place_of_the_file() {
        let mut project = Project::new(&["docs/notes.txt", COPY]).await;
        project.task.shared.resolve(COPY, Keep::Copy).await.unwrap();
        assert_eq!(
            fs::read_to_string(project.path("docs/notes.txt")).unwrap(),
            COPY
        );
        assert!(!project.path(COPY).exists());
        // The scan records the new content, and holds the copy's delete.
        project.task.scan().await;
        assert_eq!(project.held(), [COPY]);
        let listed = project
            .task
            .shared
            .lock()
            .saved
            .map
            .get("docs/notes.txt")
            .and_then(|entry| entry.file.as_ref().map(|file| file.hash));
        assert_eq!(listed, Some(*Hash::new(COPY).as_bytes()));
        project.stop().await;
    }

    /// A blob peer that takes connections and never answers on them, like
    /// one whose path stopped carrying data while the connection lives.
    #[derive(Debug, Clone)]
    struct Silent;

    impl iroh::protocol::ProtocolHandler for Silent {
        async fn accept(&self, connection: Connection) -> Result<(), iroh::protocol::AcceptError> {
            connection.closed().await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_fetch_that_gets_no_data_gives_up() {
        let project = Project::new(&["notes.txt"]).await;
        let silent = Endpoint::builder(presets::Minimal).bind().await.unwrap();
        let router = iroh::protocol::Router::builder(silent)
            .accept(iroh_blobs::ALPN, Silent)
            .spawn();
        let connection = project
            .task
            .parts
            .endpoint
            .connect(router.endpoint().addr(), iroh_blobs::ALPN)
            .await
            .unwrap();
        let content = HashAndFormat::raw(Hash::new("never sent"));
        let stall = Duration::from_millis(200);
        let stop = CancellationToken::new();
        // The guard only keeps a regression from hanging the test run.
        let fetched = timeout(
            Duration::from_secs(20),
            fetch_within(&project.task.parts.store, connection, content, stall, &stop),
        )
        .await
        .expect("the fetch gives up on its own");
        let error = fetched.expect_err("nothing was sent");
        assert!(error.to_string().contains("no data came"), "{error:#}");
        router.shutdown().await.unwrap();
        project.stop().await;
    }

    #[tokio::test]
    async fn keeping_the_original_deletes_the_copy() {
        let mut project = Project::new(&["docs/notes.txt", COPY]).await;
        project
            .task
            .shared
            .resolve(COPY, Keep::Original)
            .await
            .unwrap();
        assert!(!project.path(COPY).exists());
        assert_eq!(
            fs::read_to_string(project.path("docs/notes.txt")).unwrap(),
            "docs/notes.txt"
        );
        project.task.scan().await;
        assert_eq!(project.held(), [COPY]);
        project.stop().await;
    }

    #[tokio::test]
    async fn only_an_unchanged_conflict_copy_is_settled() {
        let project = Project::new(&["a.txt", COPY]).await;
        let shared = &project.task.shared;
        let error = shared.resolve("a.txt", Keep::Copy).await.unwrap_err();
        assert!(error.to_string().contains("not a conflict copy"), "{error}");
        let error = shared.resolve("../x (conflict 1a2b3c4d)", Keep::Copy).await;
        assert!(error.is_err());
        // Edited since the scan: left alone.
        fs::write(project.path(COPY), "edited meanwhile").unwrap();
        let error = shared.resolve(COPY, Keep::Original).await.unwrap_err();
        assert!(error.to_string().contains("changed"), "{error}");
        assert!(project.path(COPY).exists());
        project.stop().await;
    }

    #[tokio::test]
    async fn a_paused_project_says_so_and_settles_nothing() {
        let project = Project::new(&["a.txt", COPY]).await;
        project.task.shared.set_paused(true);
        project.task.publish_status(None);
        let status = project.task.shared.status();
        assert_eq!(status.state, SyncState::Paused);
        assert!(status.paused);
        assert_eq!(status.conflicts, [COPY]);
        let error = project
            .task
            .shared
            .resolve(COPY, Keep::Copy)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("paused"), "{error}");
        assert!(project.path(COPY).exists());
        project.task.shared.set_paused(false);
        project.task.publish_status(None);
        let status = project.task.shared.status();
        assert_eq!(status.state, SyncState::Idle);
        assert!(!status.paused);
        assert!(status.progress.is_none());
        project.stop().await;
    }

    #[tokio::test]
    async fn files_deleted_then_the_folder_removed_send_no_deletes() {
        let mut project = Project::new(&["a.txt", "src/b.rs"]).await;
        // `rm -rf` midway: the files are gone, the folder and marker not yet.
        fs::remove_file(project.path("a.txt")).unwrap();
        fs::remove_dir_all(project.path("src")).unwrap();
        project.task.scan().await;
        assert_eq!(project.deleted(), Vec::<String>::new(), "held, not sent");
        assert_eq!(project.held(), ["a.txt", "src/b.rs"]);

        // Then the folder goes, within the settle window.
        fs::remove_dir_all(&project.folder).unwrap();
        project.task.send_deletes().await;
        assert_eq!(project.deleted(), Vec::<String>::new());
        let error = project.task.error.clone().unwrap_or_default();
        assert!(error.contains("not syncing"), "{error}");
        project.task.scan().await;
        project.task.send_deletes().await;
        assert_eq!(project.deleted(), Vec::<String>::new());
        project.stop().await;
    }

    #[tokio::test]
    async fn the_marker_removed_with_the_files_sends_no_deletes() {
        let mut project = Project::new(&["a.txt", "b.txt"]).await;
        let marker = project.path(MARKER);
        fs::remove_file(project.path("a.txt")).unwrap();
        project.task.scan().await;
        assert_eq!(project.held(), ["a.txt"]);
        fs::remove_file(&marker).unwrap();
        fs::remove_file(project.path("b.txt")).unwrap();

        // Held deletes found the marker gone.
        project.task.send_deletes().await;
        assert_eq!(project.deleted(), Vec::<String>::new());
        assert!(project.task.scan_failed);
        // A scan does not trust the folder or mark it again.
        project.task.scan().await;
        project.task.send_deletes().await;
        assert_eq!(project.deleted(), Vec::<String>::new());
        assert!(project.task.scan_failed);
        assert!(!marker.exists(), "the marker is not written into it");
        project.stop().await;
    }

    #[tokio::test]
    async fn a_subfolder_deleted_in_a_live_project_sends_its_deletes() {
        let mut project = Project::new(&["a.txt", "sub/x.txt", "sub/deep/y.txt"]).await;
        // The marker alone removed comes back, with no file missing.
        fs::remove_file(project.path(MARKER)).unwrap();
        project.task.scan().await;
        assert_eq!(project.task.error, None);
        assert!(project.path(MARKER).exists());

        fs::remove_dir_all(project.path("sub")).unwrap();
        project.task.scan().await;
        assert_eq!(project.deleted(), Vec::<String>::new(), "held first");
        // Quiet for the window: scanned again, then sent.
        project.task.scan().await;
        project.task.send_deletes().await;
        assert_eq!(project.task.error, None);
        assert_eq!(project.deleted(), ["sub/deep/y.txt", "sub/x.txt"]);
        assert!(project.held().is_empty());
        project.stop().await;
    }

    #[tokio::test]
    async fn a_folder_moved_to_the_trash_sends_no_deletes() {
        let mut project = Project::new(&["a.txt", "b.txt"]).await;
        fs::remove_file(project.path("a.txt")).unwrap();
        project.task.scan().await;
        assert_eq!(project.held(), ["a.txt"]);
        let trash = project.base.join("Trash");
        fs::create_dir(&trash).unwrap();
        fs::rename(&project.folder, trash.join("game")).unwrap();

        project.task.send_deletes().await;
        assert_eq!(project.deleted(), Vec::<String>::new());
        assert!(project.task.scan_failed);
        project.task.scan().await;
        project.task.send_deletes().await;
        assert_eq!(project.deleted(), Vec::<String>::new());
        project.stop().await;
    }

    #[test]
    fn held_deletes_wait_until_the_folder_is_quiet() {
        let start = Instant::now();
        let mut timer = DeleteTimer::default();
        // A change with nothing held starts no wait.
        timer.changed(start);
        assert_eq!(timer.due, None);
        timer.scanned(true, start);
        assert_eq!(timer.due, Some(start + DELETE_SETTLE));
        // Another scan still holding them keeps the time; a change moves it.
        let later = start + Duration::from_secs(3);
        timer.scanned(true, later);
        assert_eq!(timer.due, Some(start + DELETE_SETTLE));
        timer.changed(later);
        assert_eq!(timer.due, Some(later + DELETE_SETTLE));
        // Nothing missing any more: nothing is due.
        timer.scanned(false, later);
        assert_eq!(timer.due, None);
    }

    fn meta(hash: u8) -> FileMeta {
        FileMeta {
            hash: [hash; 32],
            size: 1,
            mtime_ms: 1,
            executable: false,
        }
    }

    /// Entries a peer made, one per path.
    fn from_peer(paths: &[&str], hash: u8) -> Vec<Entry> {
        let mut peer = Map::new(7);
        for path in paths {
            peer.edit(path, &Version::default(), Some(meta(hash)));
        }
        peer.since(0, usize::MAX).entries
    }

    #[test]
    fn a_peer_cannot_name_paths_outside_the_folder_or_in_git() {
        let bad = [
            ".git/hooks/pre-commit",
            "src/.GIT/config",
            "../x",
            "a/../../x",
            "./a",
            "/etc/passwd",
            "",
            "a/",
            "a//b",
            "a\\b",
            "a\0b",
            ".beep-1.partial",
        ];
        let mut map = Map::new(1);
        let mut entries = from_peer(&bad, 1);
        entries.extend(from_peer(&["src/ok.rs"], 2));
        let merged = merge_peer(&mut map, &entries);
        assert_eq!(
            merged,
            Merged {
                changed: true,
                refused: bad.len()
            }
        );
        let paths: Vec<_> = map.entries().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["src/ok.rs"]);

        // Sent again, they are refused again and leave nothing to do, so
        // nothing is retried.
        let again = merge_peer(&mut map, &entries);
        assert!(!again.changed);
        let dir = tempfile::TempDir::new().unwrap();
        let state = State::new(Saved {
            map,
            ..Saved::default()
        });
        let work = plan(&state, &Rules::load(dir.path()).unwrap());
        let paths: Vec<_> = work.iter().map(|work| work.path.as_str()).collect();
        assert_eq!(paths, ["src/ok.rs"]);
    }

    #[test]
    fn a_peer_cannot_grow_the_list_past_the_file_limit() {
        let mut map = Map::new(1);
        for n in 0..MAX_FILES {
            map.edit(&format!("f/{n}"), &Version::default(), Some(meta(1)));
        }
        let entries = from_peer(&["new.txt"], 2);
        assert_eq!(merge_peer(&mut map, &entries).refused, 1);
        assert!(map.get("new.txt").is_none());
        // An edit of a file it has is still taken.
        let mut peer = Map::new(7);
        let base = map.get("f/0").unwrap().version.clone();
        peer.edit("f/0", &base, Some(meta(3)));
        let merged = merge_peer(&mut map, &peer.since(0, 10).entries);
        assert_eq!(merged.refused, 0);
        assert_eq!(map.get("f/0").unwrap().file, Some(meta(3)));
    }

    #[tokio::test]
    async fn a_file_changed_under_the_store_is_not_copied_as_its_old_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = iroh_blobs::store::fs::FsStore::load(dir.path().join("store"))
            .await
            .unwrap();
        let path = dir.path().join("a.txt");
        // Big enough that the store reads it in place, not a copy.
        fs::write(&path, vec![b'a'; 100_000]).unwrap();
        let found = Found {
            path: "a.txt".into(),
            abs: path.clone(),
            stat: Stat::of(&fs::metadata(&path).unwrap()),
        };
        let tag = scan::import(&store, &found).await.unwrap();
        let hash = tag.hash();
        let stop = CancellationToken::new();
        let copy = |name: &str| {
            let file = fs::File::create(dir.path().join(name)).unwrap();
            AsyncFile::from_std(file)
        };

        let mut out = copy("one");
        let copied = copy_store(&store, hash, &mut out, &stop).await.unwrap();
        assert_eq!(copied, *hash.as_bytes());

        // Edited in place: the same file and size, other bytes.
        let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.write_all(b"b").unwrap();
        drop(file);
        let mut out = copy("two");
        let copied = copy_store(&store, hash, &mut out, &stop).await.unwrap();
        assert_ne!(copied, *hash.as_bytes(), "the change is noticed");
        drop(tag);
        store.shutdown().await.ok();
    }

    #[test]
    fn an_emptied_folder_found_at_start_deletes_nothing() {
        let gone = |missing: &[bool]| -> Vec<(String, bool)> {
            missing
                .iter()
                .enumerate()
                .map(|(n, missing)| (format!("f/{n}"), *missing))
                .collect()
        };
        // Every file missing at the first scan: no baseline.
        assert!(!has_baseline(true, 2, &gone(&[true, true])));
        // Some still there: the rest were deleted while the node was down.
        assert!(has_baseline(true, 3, &gone(&[true, true])));
        // Left out by the rules, not missing.
        assert!(has_baseline(true, 2, &gone(&[true, false])));
        // Emptied while it runs: the deletes are real.
        assert!(has_baseline(false, 2, &gone(&[true, true])));
        assert!(has_baseline(true, 0, &[]));
    }

    #[tokio::test]
    async fn a_store_copy_that_cannot_be_read_is_fetched_again() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = iroh_blobs::store::fs::FsStore::load(dir.path().join("store"))
            .await
            .unwrap();
        let path = dir.path().join("a.txt");
        let content = vec![b'a'; 100_000];
        // Big enough that the store reads it in place, not a copy.
        fs::write(&path, &content).unwrap();
        let found = Found {
            path: "a.txt".into(),
            abs: path.clone(),
            stat: Stat::of(&fs::metadata(&path).unwrap()),
        };
        let tag = scan::import(&store, &found).await.unwrap();
        let hash = tag.hash();
        let stop = CancellationToken::new();
        // The file the store reads from is gone.
        fs::remove_file(&path).unwrap();
        let mut out = AsyncFile::from_std(fs::File::create(dir.path().join("out")).unwrap());
        assert!(copy_store(&store, hash, &mut out, &stop).await.is_err());

        let mut refetched = false;
        let mut out = AsyncFile::from_std(fs::File::create(dir.path().join("out")).unwrap());
        copy_out(
            &store,
            hash,
            &mut out,
            &stop,
            async |out: &mut AsyncFile| {
                refetched = true;
                out.set_len(0).await?;
                out.seek(SeekFrom::Start(0)).await?;
                out.write_all(&content).await?;
                Ok(())
            },
        )
        .await
        .unwrap();
        out.flush().await.unwrap();
        assert!(refetched, "a peer is asked when the store fails");
        assert_eq!(fs::read(dir.path().join("out")).unwrap(), content);
        drop(tag);
        store.shutdown().await.ok();
    }

    #[test]
    fn an_edit_after_a_won_conflict_builds_on_the_merge() {
        let stat = |inode| Stat {
            size: 1,
            mtime_ns: 0,
            ctime_ns: 0,
            inode,
            device: 1,
            executable: false,
        };
        let mut map = Map::new(1);
        let mine = map.edit("plan.md", &Version::default(), Some(meta(1)));
        // Another machine's concurrent edit loses to this one: the list
        // keeps this content under a version that has seen both.
        let mut other = Map::new(2);
        let older = FileMeta {
            mtime_ms: -1,
            ..meta(2)
        };
        let version = other.edit("plan.md", &Version::default(), Some(older.clone()));
        map.merge(&Entry {
            path: "plan.md".into(),
            version,
            file: Some(older),
        });
        let merged = map.get("plan.md").unwrap().version.clone();
        assert_eq!(merged.compare(&mine), Order::Newer);
        let mut state = State::new(Saved {
            map,
            ..Saved::default()
        });
        let seen = Seen {
            stat: stat(1),
            hash: [1; 32],
            version: mine,
        };
        state.set_seen("plan.md", seen);
        let found = Found {
            path: "plan.md".into(),
            abs: PathBuf::from("/srv/project/plan.md"),
            stat: stat(2),
        };
        let prior = state.saved.seen.get("plan.md").cloned();
        record(&mut state, &found, [3; 32], prior);
        let entry = state.saved.map.get("plan.md").unwrap();
        assert_eq!(entry.file.as_ref().map(|f| f.hash), Some([3; 32]));
        assert_eq!(entry.version.compare(&merged), Order::Newer);
        assert!(
            !state.saved.map.entries().any(|e| is_conflict(&e.path) && e.file.as_ref().is_some_and(|f| f.hash == [3; 32])),
            "the edit is not a conflict"
        );
    }

    #[test]
    fn content_never_moves_into_a_path_the_rules_skip() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let rules = Rules::load(dir.path()).unwrap();
        let mut map = Map::new(1);
        map.edit("a.bin", &Version::default(), Some(meta(2)));
        map.edit("target/a.bin", &Version::default(), Some(meta(1)));
        let stat = Stat {
            size: 1,
            mtime_ns: 0,
            ctime_ns: 0,
            inode: 1,
            device: 1,
            executable: false,
        };
        let mut state = State::new(Saved {
            map,
            ..Saved::default()
        });
        state.set_seen(
            "a.bin",
            Seen {
                stat,
                hash: [1; 32],
                version: Version::default(),
            },
        );
        assert_eq!(move_target(&state, &rules, "a.bin"), None);
        state
            .saved
            .map
            .edit("kept.bin", &Version::default(), Some(meta(1)));
        assert_eq!(
            move_target(&state, &rules, "a.bin").as_deref(),
            Some("kept.bin")
        );
    }
}
