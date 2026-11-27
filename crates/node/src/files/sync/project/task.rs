//! The project's task: its loop, and how it saves and publishes its state.

use super::{
    fetch::{Fetcher, RETRY},
    shared::Shared,
};
use crate::files::sync::{
    map::is_conflict,
    parts::Parts,
    peer::{self, SyncPool},
    progress::Meter,
    watch::{Key, Mode, RESCAN, Watcher},
};
use crate::membership::Membership;
use anyhow::Result;
use beep_proto::node::sync::State as SyncState;
use beep_proto::watched;
use iroh::EndpointAddr;
use iroh_blobs::{BlobFormat, Hash, hashseq::HashSeq};
use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::mpsc,
    task::{JoinSet, spawn_blocking},
    time::{Instant, sleep, sleep_until},
};

/// What [`Task::fail`] calls a Git check that did not pass.
pub(super) const WAITING_FOR_GIT: &str = "waiting for Git";
/// How long the folder must be quiet after a change before it is scanned.
pub(super) const SETTLE: Duration = Duration::from_secs(10);
pub(super) const MAX_SETTLE: Duration = Duration::from_secs(30);
/// How long the folder must be quiet before files a scan found missing are
/// sent as deletes. Deleting the whole folder (`rm -rf`, Finder) removes
/// its files first, and the folder itself last; the watcher reports each
/// removal as it happens, so the window only has to outlast a pause in
/// that stream (a slow disk, a descheduled `rm`), not the whole deletion.
/// Five seconds is well past such pauses and still sends an ordinary
/// delete within seconds.
pub(super) const DELETE_SETTLE: Duration = Duration::from_secs(5);
/// The longest wait before trying failed files again: each retry that
/// leaves files failing doubles the wait from `RETRY` up to this.
const MAX_RETRY: Duration = Duration::from_secs(10 * 60);
/// How often a changing state is saved; it is saved on stop as well.
const SAVE_EVERY: Duration = Duration::from_secs(2);

/// Why a scan ran, as the scan metric labels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(super) enum Reason {
    /// The project started.
    Startup,
    /// The project was resumed after a pause.
    Resume,
    /// The watcher named changed paths.
    Changes,
    /// The watcher lost events or the rules changed.
    Overflow,
    /// The node changed the folder itself.
    Touched,
    /// No watcher covers the folder, so it is checked on a timer.
    Unwatched,
    /// Someone asked for a rescan.
    Request,
    /// An earlier scan or listing failed.
    Retry,
    /// The checkout moved or lost its marker.
    Checkout,
}

/// The project's task.
pub(super) struct Task {
    pub(super) shared: Arc<Shared>,
    pub(super) parts: Parts,
    pub(super) watcher: Watcher,
    pub(super) mode: Mode,
    /// The folders the last scan watched.
    pub(super) dirs: HashSet<PathBuf>,
    /// The first scan checks the store still holds what it saw.
    pub(super) first: bool,
    /// The hashes the store's tag for the project protects now.
    protected: Option<Hash>,
    pub(super) fetcher: Arc<Fetcher>,
    pub(super) pending: u64,
    pub(super) error: Option<String>,
    /// A Git wait is held by something the watcher does not report (a Git
    /// operation, a recovery, running sessions), so it is checked on a timer.
    pub(super) git_held: bool,
    /// Repeated blocked states stay visible without logging every retry or scan.
    last_logged_error: Option<(String, Instant)>,
    /// The last scan failed, so nothing on disk is recorded.
    pub(super) scan_failed: bool,
    saved_at: Instant,
    /// Files the last scan found missing: still seen, and not deleted in
    /// the list until `send_deletes`.
    pub(super) held: BTreeSet<String>,
    /// The paths the last round could not bring, tried again later.
    pub(super) waiting: Vec<String>,
}

/// When held deletes are due: once the folder was quiet for
/// `DELETE_SETTLE`. Times are passed in, so it runs on any clock.
#[derive(Debug, Default)]
pub(super) struct DeleteTimer {
    pub(super) due: Option<Instant>,
}

#[derive(Debug, Default)]
pub(super) struct ChangeTimer {
    first: Option<Instant>,
    pub(super) due: Option<Instant>,
}

impl ChangeTimer {
    pub(super) fn changed(&mut self, now: Instant) {
        let first = *self.first.get_or_insert(now);
        self.due = Some((now + SETTLE).min(first + MAX_SETTLE));
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
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

/// Next retry wait after a scan finishes: backs off up to `MAX_RETRY` while
/// scanning still fails, and resets to `RETRY` once a scan succeeds.
pub(super) fn next_scan_retry(retry: Duration, scan_failed: bool) -> Duration {
    if scan_failed {
        (retry * 2).min(MAX_RETRY)
    } else {
        RETRY
    }
}

/// Runs the project until it stops: scans the folder when the watcher says
/// it changed (or on a timer), records the edits in the list, then applies
/// the list to the folder. Pulls from peers merge into the list from their
/// own tasks and wake this one.
pub(in crate::files::sync) async fn run(
    shared: Arc<Shared>,
    parts: Parts,
    watcher: Watcher,
    pool: SyncPool,
    mesh: Membership,
) {
    let (wake, mut woken) = mpsc::channel(1);
    watcher.add(shared.key, shared.root(), shared.rules.clone(), wake);
    // The folder itself is watched before the first walk, so a change
    // made during it is not missed; the walk adds the rest.
    let first_mode = watcher
        .set_dirs(shared.key, vec![shared.root().to_owned()])
        .await;
    let mut task = Task::new(shared.clone(), parts, watcher.clone(), first_mode);
    let mut peers = shared.peers.subscribe();
    let mut peer_commits = shared.peer_commits.subscribe();
    let mut list = shared.subscribe();
    let mut paused = shared.paused.subscribe();
    let mut pulls = JoinSet::new();
    // Scanned before anything is pulled, so edits made while the node was
    // down merge as edits. A paused project does this once resumed.
    if !watched::read_and_update(&mut paused, |paused| *paused) {
        task.scan(Reason::Startup).await;
        task.apply().await;
        task.save(true).await;
        let current = watched::read_and_update(&mut peers, Clone::clone);
        start_pulls(&mut pulls, &shared, &task.parts, &pool, &mesh, &current);
    }

    let mut settle = ChangeTimer::default();
    let mut full = false;
    let mut deletes = DeleteTimer::default();
    deletes.scanned(!task.held.is_empty(), Instant::now());
    let mut last_scan = Instant::now();
    let mut retry = RETRY;

    loop {
        if watched::read_and_update(&mut paused, |paused| *paused) {
            // Nothing moves: the pulls stop, and changes on disk wait for
            // the scan on resume.
            pulls = JoinSet::new();
            settle.clear();
            full = false;
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

            if !watched::read(&paused, |paused| *paused) {
                while woken.try_recv().is_ok() {}
                shared.listen_again();
                watcher.take_changes(shared.key);
                task.scan(Reason::Resume).await;
                last_scan = Instant::now();
                task.apply().await;
                deletes.scanned(!task.held.is_empty(), last_scan);
                let current = watched::read_and_update(&mut peers, Clone::clone);
                start_pulls(&mut pulls, &shared, &task.parts, &pool, &mesh, &current);
            }
            continue;
        }

        let mut apply = false;
        let mut forced = false;
        // A change waiting to be saved is saved once `SAVE_EVERY` passed.
        let unsaved = shared.lock().unsaved;
        tokio::select! {
            () = shared.stop.cancelled() => break,
            Some(()) = woken.recv() => {
                let now = Instant::now();
                settle.changed(now);
                deletes.changed(now);
            }
            () = shared.touched.notified() => {
                let now = Instant::now();
                full = true;
                settle.changed(now);
                deletes.changed(now);
            }
            changed = paused.changed() => if changed.is_err() {
                break;
            },
            () = sleep_until(settle.due.unwrap_or_else(Instant::now)), if settle.due.is_some() => {
                settle.clear();
                let changes = watcher.take_changes(shared.key);
                let touched = std::mem::take(&mut full);
                if changes.full {
                    task.scan(Reason::Overflow).await;
                } else if touched {
                    task.scan(Reason::Touched).await;
                } else {
                    task.scan_changes(changes.paths).await;
                }
                last_scan = Instant::now();
                apply = true;
            }
            () = sleep_until(deletes.due.unwrap_or_else(Instant::now)), if deletes.due.is_some() && settle.due.is_none() => {
                deletes.due = None;
                // Quiet long enough: scanned again, and what is still
                // missing is sent unless the folder changed meanwhile.
                let changes = watcher.take_changes(shared.key);
                if changes.full {
                    task.scan(Reason::Overflow).await;
                } else {
                    task.scan_changes(changes.paths).await;
                }
                last_scan = Instant::now();
                if woken.try_recv().is_ok() {
                    settle.changed(last_scan);
                } else {
                    task.send_deletes().await;
                }
                apply = true;
            }
            () = sleep_until(last_scan + RESCAN), if !matches!(task.mode, Mode::Watching) => {
                task.scan(Reason::Unwatched).await;
                last_scan = Instant::now();
                apply = true;
            }
            () = shared.forced.notified() => {
                settle.clear();
                full = false;
                watcher.take_changes(shared.key);
                task.scan(Reason::Request).await;
                last_scan = Instant::now();
                apply = true;
                forced = true;
            }
            () = shared.matching.notified() => {
                settle.clear();
                let changes = watcher.take_changes(shared.key);
                let touched = std::mem::take(&mut full);

                if task.scan_for_match(changes, touched).await {
                    last_scan = Instant::now();
                    apply = true;
                }

                forced = true;
            }
            () = shared.merged.notified() => {
                // While only a change clears a Git wait, a merge that left
                // the list as it was cannot; a broken root still waits on its scan.
                if !task.scan_failed && (!task.waits_for_change() || list.has_changed().unwrap_or(true)) {
                    retry = RETRY;
                    apply = true;
                }
            }
            Ok(()) = peer_commits.changed() => {
                if !task.scan_failed {
                    retry = RETRY;
                    apply = true;
                }
            }
            // A Git wait is cleared by a change; edits thrown away by hand
            // are not one, so it is still checked now and then.
            () = sleep(if task.waits_for_change() { MAX_RETRY } else { retry }), if task.pending > 0 => {
                retry = (retry * 2).min(MAX_RETRY);
                apply = true;
            }
            () = sleep_until(last_scan + retry), if task.scan_failed => {
                task.scan(Reason::Retry).await;
                last_scan = Instant::now();
                retry = next_scan_retry(retry, task.scan_failed);
                if !task.scan_failed {
                    apply = true;
                }
            }
            () = sleep_until(task.saved_at + SAVE_EVERY), if unsaved => {}
            changed = peers.changed() => {
                if changed.is_err() {
                    break;
                }
                pulls = JoinSet::new();
                shared.listen_again();
                let current = watched::read_and_update(&mut peers, Clone::clone);
                start_pulls(&mut pulls, &shared, &task.parts, &pool, &mesh, &current);
                if !task.scan_failed {
                    retry = RETRY;
                    apply = true;
                }
            }
            Some(ended) = pulls.join_next() => if let Err(e) = ended {
                log::warn!("a pull of {} failed: {e}", shared.name);
            },
        }
        deletes.scanned(!task.held.is_empty(), Instant::now());

        if apply && settle.due.is_none() {
            let waited = task.waits_for_git();
            list.mark_unchanged();
            task.apply().await;

            if task.pending == 0 && !task.scan_failed {
                retry = RETRY;
            }
            // Pulls that failed on the Git wait dial again now, not after
            // their pause.
            if waited && !task.waits_for_git() {
                shared.pull_again();
            }
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
    pool: &SyncPool,
    mesh: &Membership,
    peers: &[EndpointAddr],
) {
    for peer in peers {
        pulls.spawn(peer::pull(
            shared.clone(),
            parts.endpoint.clone(),
            pool.clone(),
            mesh.clone(),
            peer.clone(),
        ));
    }
}

impl Task {
    pub(super) fn new(shared: Arc<Shared>, parts: Parts, watcher: Watcher, mode: Mode) -> Self {
        Self {
            fetcher: Arc::new(Fetcher::new(shared.clone(), parts.clone())),
            shared,
            parts,
            watcher,
            mode,
            dirs: HashSet::new(),
            first: true,
            protected: None,
            pending: 0,
            error: None,
            git_held: false,
            last_logged_error: None,
            scan_failed: false,
            saved_at: Instant::now(),
            held: BTreeSet::new(),
            waiting: Vec::new(),
        }
    }

    pub(super) fn fail(&mut self, what: &str, e: &anyhow::Error) {
        let message = format!("{what}: {e:#}");
        let now = Instant::now();
        if self.last_logged_error.as_ref().is_none_or(|(last, at)| {
            last != &message || now.duration_since(*at) >= Duration::from_secs(60)
        }) {
            log::warn!("{}: {message}", self.shared.name);
            self.last_logged_error = Some((message.clone(), now));
        }
        self.error = Some(message);
    }

    /// Whether the error is a Git check's, which only a Git check clears.
    pub(super) fn waits_for_git(&self) -> bool {
        self.error
            .as_deref()
            .and_then(|error| error.strip_prefix(WAITING_FOR_GIT))
            .is_some_and(|rest| rest.starts_with(": "))
    }

    /// Whether only a change clears the Git wait: the checkout moving, which
    /// the watcher reports, or a peer's commits, which the pulls report.
    pub(super) fn waits_for_change(&self) -> bool {
        self.waits_for_git() && !self.git_held
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
    pub(super) async fn save(&mut self, now: bool) -> bool {
        if !now && self.saved_at.elapsed() < SAVE_EVERY {
            return true;
        }

        let Some(snapshot) = self.shared.lock().snapshot() else {
            return true;
        };
        self.saved_at = Instant::now();
        let file = self.shared.file.clone();
        let seq = snapshot.map.seq();
        let written = spawn_blocking(move || {
            let bytes = rmp_serde::to_vec(&snapshot)?;
            crate::fs::note::write(&file, &bytes)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|written| written);

        if let Err(e) = written {
            log::warn!("could not save {}: {e:#}", self.shared.name);
            // Tried again on the next pass.
            self.shared.lock().unsaved = true;
            return false;
        }

        self.shared.saved_seq.send_if_modified(|saved| {
            if *saved == seq {
                return false;
            }

            *saved = seq;
            true
        });
        true
    }

    /// Publishes how far the round is, and what still waits.
    pub(super) fn show(&self, meter: &Meter, waiting: Vec<String>) {
        let progress = meter.progress();
        self.shared.publish(move |status| {
            status.progress = Some(progress);
            status.waiting = waiting;
        });
    }

    /// Publishes how far the round is, leaving what waits as it was.
    pub(super) fn show_progress(&self, meter: &Meter) {
        let progress = meter.progress();
        self.shared
            .publish(move |status| status.progress = Some(progress));
    }

    /// Publishes how the project stands; progress shows only while a round
    /// runs, and `show` sets it then.
    pub(super) fn publish_status(&self, state: Option<SyncState>) {
        let git_only = self.shared.lock().saved.git_only;
        let (files, bytes, conflicts) = {
            let locked = self.shared.lock();
            let bytes = locked.saved.seen.values().map(|seen| seen.stat.size).sum();
            let conflicts = locked
                .saved
                .map
                .entries()
                .filter(|entry| !git_only && entry.file.is_some() && is_conflict(&entry.path))
                .map(|entry| entry.path.clone())
                .collect();
            (locked.saved.seen.len() as u64, bytes, conflicts)
        };
        let paused = self.shared.is_paused();
        let state = state.unwrap_or(if paused {
            SyncState::Paused
        } else if self.scan_failed || git_only && self.error.is_some() {
            SyncState::Failed
        } else if self.pending > 0 || !self.shared.heard_all() {
            SyncState::Syncing
        } else {
            SyncState::Idle
        });
        let (rescan_secs, hint) = match &self.mode {
            Mode::Watching => (None, None),
            Mode::Rescan { hint } => (Some(RESCAN.as_secs() as u32), Some((*hint).to_owned())),
        };
        let pending = self.pending;
        let error = self.error.clone();
        let waiting = self.waiting.clone();
        self.shared.publish(move |status| {
            status.git_only = git_only;
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
