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
    map::{Changes, Entry, FileMeta, Map, Replica, Version, is_conflict, valid_path},
    peer,
    root::{self, RootId},
    rules::Rules,
    scan::{self, Found, MAX_BYTES, MAX_FILES, Stat},
    watch::{Key, Mode, RESCAN, Watcher},
};
use crate::transfer::place::{Folder, Partial, Reserved, create_file_at, space_at};
use anyhow::{Context, Result, anyhow, bail};
use bao_tree::io::BaoContentItem;
use beep_proto::node::sync::{ProjectStatus, State as SyncState};
use futures::StreamExt as _;
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use iroh_blobs::{
    BlobFormat, Hash, HashAndFormat,
    api::{Store, TempTag},
    get::request::{GetBlobItem, get_blob},
    hashseq::HashSeq,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsString,
    fs,
    io::{SeekFrom, Write as _},
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
/// How often a watched folder is scanned anyway, for changes a watcher
/// missed.
const FULL_RESCAN: Duration = Duration::from_secs(10 * 60);
/// How soon files that could not be fetched are tried again.
const RETRY: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// What every disk keeps free beyond a fetch.
const SPACE_MARGIN: u64 = 64 << 20;
const CHUNK: usize = 1 << 20;

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
#[derive(Debug, Default, Serialize, Deserialize)]
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
    /// The folder is not the one synced before, so nothing is written.
    root_failed: bool,
}

/// Runs the project until it stops.
pub(super) async fn run(shared: Arc<Shared>, parts: Parts, watcher: Watcher) {
    let (wake, mut woken) = mpsc::channel(1);
    watcher.add(shared.key, shared.root(), shared.rules.clone(), wake);
    // The folder itself is watched before the first walk, so a change
    // made during it is not missed; the walk adds the rest.
    let first_mode = watcher.set_dirs(shared.key, &[shared.root().to_owned()]);
    let mut task = Task {
        shared: shared.clone(),
        parts,
        watcher: watcher.clone(),
        mode: first_mode,
        first: true,
        protected: None,
        blobs: HashMap::new(),
        pending: 0,
        error: None,
        scan_failed: false,
        root_failed: false,
    };
    let mut peers = shared.peers.subscribe();
    let mut pulls = JoinSet::new();
    // Scanned before anything is pulled, so edits made while the node was
    // down merge as edits.
    task.scan().await;
    task.apply().await;
    task.save().await;
    start_pulls(&mut pulls, &shared, &task.parts, &peers.borrow_and_update());
    let mut settle: Option<Instant> = None;
    let mut last_scan = Instant::now();
    loop {
        let every = match task.mode {
            Mode::Watching => FULL_RESCAN,
            Mode::Rescan { .. } => RESCAN,
        };
        let mut apply = false;
        tokio::select! {
            () = shared.stop.cancelled() => break,
            Some(()) = woken.recv() => {
                settle.get_or_insert_with(|| Instant::now() + SETTLE);
            }
            () = sleep_until(settle.unwrap_or_else(Instant::now)), if settle.is_some() => {
                settle = None;
                task.scan().await;
                last_scan = Instant::now();
                apply = true;
            }
            () = sleep_until(last_scan + every) => {
                task.scan().await;
                last_scan = Instant::now();
                apply = true;
            }
            () = shared.merged.notified() => apply = true,
            () = sleep(RETRY), if task.pending > 0 => apply = true,
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
        if apply && settle.is_none() {
            task.apply().await;
        }
        task.save().await;
    }
    watcher.remove(shared.key);
    task.save().await;
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
        match result {
            Ok(()) => self.error = None,
            Err(e) => self.fail("scanning failed", &e),
        }
        self.publish_status(None);
    }

    async fn try_scan(&mut self) -> Result<()> {
        self.check_root().await?;
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
        self.mode = self.watcher.set_dirs(self.shared.key, &walked.dirs);
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
        self.first = false;
        let kept: HashSet<&str> = walked.files.iter().map(|f| f.path.as_str()).collect();
        let gone: Vec<String> = self
            .shared
            .lock()
            .saved
            .seen
            .keys()
            .filter(|path| !kept.contains(path.as_str()))
            .cloned()
            .collect();
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
        let (before, seq) = {
            let mut state = self.shared.lock();
            let before = state.saved.map.seq();
            for (found, hash, prior) in hashed {
                record(&mut state, found, hash, prior);
            }
            for (path, missing) in deleted {
                let Some(old) = state.remove_seen(&path) else {
                    continue;
                };
                if missing {
                    state.saved.map.edit(&path, &old.version, None);
                }
            }
            (before, state.saved.map.seq())
        };
        if seq != before {
            self.shared.seq.send_replace(seq);
        }
        self.protect().await?;
        drop(tags);
        Ok(())
    }

    /// Checks the folder is the one synced before; until it is, nothing
    /// is scanned or written.
    async fn check_root(&mut self) -> Result<()> {
        let (saved, seen_any) = {
            let state = self.shared.lock();
            (state.saved.root.clone(), !state.saved.seen.is_empty())
        };
        let folder = self.shared.root.clone();
        let checked =
            spawn_blocking(move || root::check(&folder, saved.as_ref(), seen_any)).await?;
        self.root_failed = checked.is_err();
        let id = checked?;
        let mut state = self.shared.lock();
        if state.saved.root.as_ref() != Some(&id) {
            state.saved.root = Some(id);
            state.unsaved = true;
        }
        Ok(())
    }

    /// Brings the folder in line with the list.
    async fn apply(&mut self) {
        if self.root_failed {
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
            self.publish_status(None);
            return;
        }
        self.publish_status(Some(SyncState::Syncing));
        let mut partial = None;
        let mut failed = 0;
        let total = work.len();
        for item in work {
            if self.shared.stop.is_cancelled() {
                break;
            }
            if let Err(e) = self.bring(item, &mut partial).await {
                failed += 1;
                self.fail("syncing a file failed", &e);
            }
        }
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
        let copied = copy_store(&self.parts.store, hash, &mut out, &self.shared.stop).await?;
        if copied != file.hash {
            // The store reads it from a file in place, which was changed
            // since; a peer sends it again, checked as it comes.
            log::info!(
                "{}: the copy of {} on this machine changed; fetching it again",
                self.shared.name,
                work.path
            );
            self.copy_from_peers(hash, &mut out).await?;
        }
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
            let fetched = tokio::select! {
                fetched = self.parts.store.remote().fetch(connection, content) => fetched,
                () = self.shared.stop.cancelled() => bail!("stopped"),
            };
            match fetched {
                Ok(_) if self.parts.store.blobs().has(content.hash).await? => {
                    return Ok(Fetched {
                        _tag: tag,
                        _room: room,
                    });
                }
                Ok(_) => last = Some(anyhow!("{} sent only part of it", peer.id)),
                Err(e) => {
                    self.blobs.remove(&peer.id);
                    last = Some(anyhow!("{}: {e}", peer.id));
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

    async fn save(&mut self) {
        let bytes = {
            let mut state = self.shared.lock();
            if !state.unsaved {
                return;
            }
            state.unsaved = false;
            rmp_serde::to_vec_named(&state.saved)
        };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(e) => return log::warn!("could not save {}: {e}", self.shared.name),
        };
        let file = self.shared.file.clone();
        match spawn_blocking(move || write_atomic(&file, &bytes)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("could not save {}: {e:#}", self.shared.name),
            Err(e) => log::warn!("could not save {}: {e}", self.shared.name),
        }
    }

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
        let state = state.unwrap_or(if self.scan_failed {
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
        self.shared.publish(move |status| {
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
            item = items.next() => item,
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
        Some(prior) => state
            .saved
            .map
            .edit(&found.path, &prior.version, Some(meta)),
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

    #[test]
    fn the_tag_name_is_stable() {
        assert_eq!(tag_name(0xab), "sync/00000000000000ab");
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
