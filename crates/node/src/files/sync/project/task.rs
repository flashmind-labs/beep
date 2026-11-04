//! The project's task: its loop, and how it saves and publishes its state.

use super::shared::Shared;
use crate::files::sync::{
    Parts,
    map::is_conflict,
    peer,
    progress::Meter,
    watch::{Key, Mode, RESCAN, Watcher},
};
use anyhow::Result;
use beep_proto::node::sync::State as SyncState;
use iroh::{EndpointAddr, EndpointId, endpoint::Connection};
use iroh_blobs::{BlobFormat, Hash, api::TempTag, hashseq::HashSeq};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::mpsc,
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, sleep_until},
};

/// How long the folder must be quiet after a change before it is scanned.
const SETTLE: Duration = Duration::from_millis(300);
/// How long the folder must be quiet before files a scan found missing are
/// sent as deletes. Deleting the whole folder (`rm -rf`, Finder) removes
/// its files first, and the folder itself last; the watcher reports each
/// removal as it happens, so the window only has to outlast a pause in
/// that stream (a slow disk, a descheduled `rm`), not the whole deletion.
/// Five seconds is well past such pauses and still sends an ordinary
/// delete within seconds.
pub(super) const DELETE_SETTLE: Duration = Duration::from_secs(5);
/// How often a watched folder is scanned anyway, for changes a watcher
/// missed.
const FULL_RESCAN: Duration = Duration::from_secs(10 * 60);
/// How soon files that could not be fetched are tried again.
const RETRY: Duration = Duration::from_secs(30);
/// How often a changing state is saved; it is saved on stop as well.
const SAVE_EVERY: Duration = Duration::from_secs(2);

/// The project's task.
pub(super) struct Task {
    pub(super) shared: Arc<Shared>,
    pub(super) parts: Parts,
    pub(super) watcher: Watcher,
    pub(super) mode: Mode,
    /// The first scan checks the store still holds what it saw.
    pub(super) first: bool,
    /// The hashes the store's tag for the project protects now.
    protected: Option<Hash>,
    pub(super) blobs: HashMap<EndpointId, Connection>,
    pub(super) pending: u64,
    pub(super) error: Option<String>,
    /// The last scan failed, so nothing on disk is recorded.
    pub(super) scan_failed: bool,
    saved_at: Instant,
    /// Files the last scan found missing: still seen, and not deleted in
    /// the list until `send_deletes`.
    pub(super) held: BTreeSet<String>,
    /// The paths the last round could not bring, tried again later.
    pub(super) waiting: Vec<String>,
    /// Tags on what those fetches got so far: without one, the store's
    /// collector deletes it between rounds and the file starts over.
    pub(super) unfinished: HashMap<Hash, TempTag>,
}

/// When held deletes are due: once the folder was quiet for
/// `DELETE_SETTLE`. Times are passed in, so it runs on any clock.
#[derive(Debug, Default)]
pub(super) struct DeleteTimer {
    pub(super) due: Option<Instant>,
}

impl DeleteTimer {
    /// The folder changed at `now`: held deletes wait again.
    pub(super) fn changed(&mut self, now: Instant) {
        if let Some(due) = &mut self.due {
            *due = now + DELETE_SETTLE;
        }
    }

    /// A scan ended at `now`, holding deletes or not.
    pub(super) fn scanned(&mut self, holds: bool, now: Instant) {
        if !holds {
            self.due = None;
        } else if self.due.is_none() {
            self.due = Some(now + DELETE_SETTLE);
        }
    }
}

/// Runs the project until it stops: scans the folder when the watcher says
/// it changed (or on a timer), records the edits in the list, then applies
/// the list to the folder. Pulls from peers merge into the list from their
/// own tasks and wake this one.
pub(in crate::files::sync) async fn run(shared: Arc<Shared>, parts: Parts, watcher: Watcher) {
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
    pub(super) fn new(shared: Arc<Shared>, parts: Parts, watcher: Watcher, mode: Mode) -> Self {
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
            unfinished: HashMap::new(),
        }
    }

    pub(super) fn fail(&mut self, what: &str, e: &anyhow::Error) {
        let message = format!("{what}: {e:#}");
        log::warn!("{}: {message}", self.shared.name);
        self.error = Some(message);
    }

    /// Keeps what the folder holds in the store, for peers to fetch, under
    /// one tag for the project.
    pub(super) async fn protect(&mut self) -> Result<()> {
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
            crate::link::note::write(&file, &bytes)
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
    pub(super) fn show(&self, meter: &Meter, waiting: Vec<String>) {
        let progress = meter.progress();
        self.shared.publish(move |status| {
            status.progress = Some(progress);
            status.waiting = waiting;
        });
    }

    /// Publishes how the project stands; progress shows only while a round
    /// runs, and `show` sets it then.
    pub(super) fn publish_status(&self, state: Option<SyncState>) {
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

/// The name of the store tag that keeps project `key`'s files.
pub(in crate::files::sync) fn tag_name(key: Key) -> String {
    format!("sync/{key:016x}")
}
