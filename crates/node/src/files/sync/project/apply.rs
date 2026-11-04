//! Bringing the folder in line with the list.

use super::{
    fetch::{Ahead, Fetched, Wanted, copy_out},
    list::{Work, move_target, plan, turn},
    state::Seen,
    task::Task,
};
use crate::files::sync::{
    disk::{self},
    gitdir::{self, Kind, Oid, RefMove, Repo, Stay, ref_stat},
    map::{FileMeta, is_conflict, original_of},
    progress::{self, Meter},
    root::{Checked, exclude_from_git},
    rules::MARKER,
    scan::{self, Found, Stat},
};
use crate::files::{fsat::Dir, transfer::place::Folder};
use crate::{files::transfer::place::Partial, read_lock};
use anyhow::{Context, Result, anyhow, bail};
use beep_proto::node::sync::{MAX_WAITING, State as SyncState};
use iroh_blobs::Hash;
use std::{
    collections::{HashSet, VecDeque},
    ffi::OsString,
    mem,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    fs::File as AsyncFile,
    io::AsyncWriteExt,
    task::spawn_blocking,
    time::{Instant, sleep_until},
};

/// How often progress is published while files are brought in line.
const SHOW_EVERY: Duration = Duration::from_millis(500);
/// The most files, bytes and time written files wait to go in place, all
/// reaching the disk at one flush. One flush per file costs up to 70 ms on a
/// disk that is slow to flush, and one per thousand about 400 ms.
const BATCH_FILES: usize = 1000;
const BATCH_BYTES: u64 = 64 << 20;
const BATCH_AGE: Duration = Duration::from_secs(1);

/// A file written into the partial folder, waiting to go in place.
struct Sealed {
    temp: OsString,
    work: Work,
    hash: [u8; 32],
    size: u64,
    /// Keeps the blob from the collector until the file counts as here.
    _fetched: Option<Fetched>,
}

/// How a round stands.
struct Round {
    partial: Option<Partial>,
    /// Written, and not yet in place or on the list as seen.
    batch: Vec<Sealed>,
    batch_bytes: u64,
    oldest: Instant,
    turn: u8,
    /// Something was removed or moved since the last flush.
    unsynced: bool,
    failed: u64,
    lost: Vec<String>,
    meter: Meter,
    waited: Duration,
    wrote: Duration,
    flushed: Duration,
}

impl Round {
    fn is_due(&self) -> bool {
        self.batch.len() >= BATCH_FILES
            || self.batch_bytes >= BATCH_BYTES
            || (!self.batch.is_empty() && self.oldest.elapsed() >= BATCH_AGE)
    }
}

/// Puts `batch` in place, durably: its content reaches the disk before any
/// rename, and the renames before anything records them. A crash leaves each
/// file either unseen in the partial folder or whole at its path, and the
/// state on disk never says a file is here that may not be.
fn place_all(root: &Folder, from: &Dir, batch: &[Sealed]) -> Result<Vec<Result<Stat>>> {
    from.sync_disk().context("writing to the disk")?;
    let placed = batch
        .iter()
        .map(|sealed| {
            let (path, expected) = (&sealed.work.path, sealed.work.expected.as_ref());
            disk::place(root, from, &sealed.temp, path, expected)
        })
        .collect();
    from.sync_disk().context("writing to the disk")?;
    Ok(placed)
}

/// A ref kept for the losing side of two commits, and the branch it lost.
struct Pair {
    copy: String,
    branch: String,
    lost: FileMeta,
    won: [u8; 32],
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_millis()).ok())
        .unwrap_or(0)
}

impl Task {
    /// Brings the folder in line with the list.
    pub(super) async fn apply(&mut self) {
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
            let rules = read_lock(&self.shared.rules);
            plan(&state, &rules)
        };
        let wanted: HashSet<Hash> = work
            .iter()
            .filter_map(|item| item.file.as_ref())
            .map(|file| Hash::from_bytes(file.hash))
            .collect();
        self.fetcher.keep_only(&wanted);
        if work.is_empty() {
            self.pending = 0;
            self.waiting.clear();
            self.finish_git().await;
            self.publish_status(None);
            return;
        }
        self.publish_status(Some(SyncState::Syncing));
        let total = work.len();
        let paths: Vec<String> = work.iter().map(|item| item.path.clone()).collect();
        let bytes = work
            .iter()
            .filter_map(|item| item.file.as_ref())
            .map(|file| file.size)
            .sum();
        let fetches: VecDeque<Wanted> = work
            .iter()
            .filter(|item| !gitdir::is_ref(&item.path))
            .filter_map(|item| {
                item.file.clone().map(|file| Wanted {
                    path: item.path.clone(),
                    file,
                })
            })
            .collect();
        let mut ahead = Ahead::new(self.fetcher.clone(), fetches);
        let mut round = Round {
            partial: None,
            batch: Vec::new(),
            batch_bytes: 0,
            oldest: Instant::now(),
            turn: 0,
            unsynced: false,
            failed: 0,
            lost: Vec::new(),
            meter: Meter::new(total as u64, bytes, Instant::now()),
            waited: Duration::ZERO,
            wrote: Duration::ZERO,
            flushed: Duration::ZERO,
        };
        self.show(&round.meter, progress::waiting(&round.lost, &paths));
        let mut shown = Instant::now();
        for (at, item) in work.into_iter().enumerate() {
            if self.shared.stop.is_cancelled() || self.shared.is_paused() {
                break;
            }
            // What a turn writes is on disk before the next turn starts.
            if turn(&item) != round.turn {
                round.turn = turn(&item);
                self.flush(&mut round).await;
            }
            let size = item.file.as_ref().map_or(0, |file| file.size);
            let hash = item.file.as_ref().map(|file| Hash::from_bytes(file.hash));
            let path = item.path.clone();
            // HEAD moves only once all it may name is here.
            let brought = if round.failed > 0 && gitdir::kind(&path) == Some(Kind::Head) {
                Err(anyhow!("waiting for the rest of the repository"))
            } else {
                self.bring(item, &mut round, &mut ahead).await
            };
            if let Some(hash) = hash {
                ahead.forget(&hash);
            }
            match brought {
                Ok(true) => round.meter.done(size, Instant::now()),
                Ok(false) => {}
                Err(e) => self.note_failure(&mut round, path, size, &e),
            }
            if round.is_due() {
                self.flush(&mut round).await;
            }
            if shown.elapsed() >= SHOW_EVERY {
                shown = Instant::now();
                let rest = paths.get(at + 1..).unwrap_or_default();
                self.show(&round.meter, progress::waiting(&round.lost, rest));
            }
        }
        // Stops the fetches. What they got stays for the next round.
        drop(ahead);
        self.flush(&mut round).await;
        if round.unsynced {
            let root = self.shared.root.clone();
            let synced = spawn_blocking(move || root.dir().sync_disk()).await;
            if let Ok(Err(e)) = synced.as_ref() {
                round.failed += 1;
                self.fail("writing to the disk failed", &anyhow!("{e}"));
            }
        }
        self.waiting = mem::take(&mut round.lost);
        if let Some(partial) = round.partial.take() {
            let finished = spawn_blocking(move || partial.finish()).await;
            if let Ok(Err(e)) | Err(e) = finished.map_err(anyhow::Error::from) {
                log::warn!("a partial folder stays until the next start: {e:#}");
            }
        }
        if round.failed == 0 {
            self.error = None;
        }
        log::debug!(
            "{}: {total} changes, {} failed; waited {:?} for fetches, wrote for {:?}, flushed for {:?}",
            self.shared.name,
            round.failed,
            round.waited,
            round.wrote,
            round.flushed,
        );
        self.pending = round.failed;
        self.finish_git().await;
        if let Err(e) = self.protect().await {
            self.fail("keeping the files in the store failed", &e);
        }
        self.publish_status(None);
    }

    fn note_failure(&mut self, round: &mut Round, path: String, size: u64, e: &anyhow::Error) {
        // The first failure says why; what follows may only be waiting for it.
        if round.failed == 0 {
            self.fail("syncing a file failed", e);
        } else {
            log::debug!("{}: {path}: {e:#}", self.shared.name);
        }
        round.failed += 1;
        round.meter.failed(size);
        if round.lost.len() < MAX_WAITING {
            round.lost.push(path);
        }
    }

    /// Puts the batch in place and counts its files as here.
    async fn flush(&mut self, round: &mut Round) {
        let Some(partial) = round.partial.as_ref().filter(|_| !round.batch.is_empty()) else {
            return;
        };
        let started = Instant::now();
        let batch = mem::take(&mut round.batch);
        round.batch_bytes = 0;
        let (root, dir) = (self.shared.root.clone(), partial.dir().try_clone());
        let placed = spawn_blocking(move || {
            let placed = dir
                .map_err(anyhow::Error::from)
                .and_then(|dir| place_all(&root, &dir, &batch));
            (batch, placed)
        })
        .await;
        round.flushed += started.elapsed();
        let (batch, placed) = match placed {
            Ok(done) => done,
            Err(e) => {
                self.fail("putting files in place did not finish", &e.into());
                round.failed += 1;
                return;
            }
        };
        let placed = match placed {
            Ok(placed) => placed,
            Err(e) => {
                for sealed in batch {
                    self.note_failure(round, sealed.work.path, sealed.size, &e);
                }
                return;
            }
        };
        for (sealed, placed) in batch.into_iter().zip(placed) {
            match placed {
                Ok(stat) => {
                    let head = gitdir::kind(&sealed.work.path) == Some(Kind::Head);
                    let mut state = self.shared.lock();
                    state.set_seen(
                        &sealed.work.path,
                        Seen {
                            stat,
                            hash: sealed.hash,
                            version: sealed.work.version,
                        },
                    );
                    state.saved.index_stale |= head;
                    drop(state);
                    round.meter.done(sealed.size, Instant::now());
                }
                Err(e) => self.note_failure(round, sealed.work.path, sealed.size, &e),
            }
        }
    }

    /// Brings one path in line, unless the list or the disk moved on since
    /// it was planned. `false` when the file waits in the batch.
    async fn bring(&mut self, work: Work, round: &mut Round, ahead: &mut Ahead) -> Result<bool> {
        if gitdir::is_ref(&work.path) {
            round.unsynced = true;
            self.bring_ref(work).await?;
            return Ok(true);
        }
        let kind = gitdir::kind(&work.path);
        if kind == Some(Kind::Head) {
            let root = self.shared.root().to_owned();
            if spawn_blocking(move || gitdir::is_busy(&root)).await? {
                bail!("waiting for the git operation running on this machine");
            }
        }
        // What the folder holds elsewhere may make a fetch unneeded: the
        // losing side of a conflict, here at `path`, moves to its copy.
        if let Some(expected) = work.expected
            && let Some(keep_as) = self.moves_to(&work.path)
        {
            self.flush(round).await;
            round.unsynced = true;
            self.keep_as(&work.path, &keep_as, expected).await?;
            if work.file.is_none() {
                return Ok(true);
            }
            let work = Work {
                expected: None,
                ..work
            };
            return Box::pin(self.bring(work, round, ahead)).await;
        }
        let Some(file) = work.file.clone() else {
            round.unsynced = true;
            self.delete(&work).await?;
            return Ok(true);
        };
        if !self.still_wanted(&work) {
            return Ok(true);
        }
        let hash = Hash::from_bytes(file.hash);
        let fetched = self.blob_from(ahead, hash, round).await?;
        let started = Instant::now();
        let partial = match &round.partial {
            Some(partial) => partial,
            None => {
                let (journal, root) = (self.parts.journal.clone(), self.shared.root.clone());
                let started = spawn_blocking(move || journal.start(root)).await??;
                round.partial.insert(started)
            }
        };
        let temp = OsString::from(format!("{:x}", uuid::Uuid::new_v4().as_u64_pair().0));
        let created = {
            let (dir, temp, executable) =
                (partial.dir().try_clone()?, temp.clone(), file.executable);
            spawn_blocking(move || dir.create_file(&temp, executable)).await??
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
                self.fetcher.copy_from_peers(hash, out).await
            },
        )
        .await?;
        out.flush().await?;
        let out = out.into_std().await;
        // An object must not look old enough for this machine's next
        // `git gc` to prune it before the ref that names it comes.
        let mtime = if kind == Some(Kind::Object) {
            now_ms()
        } else {
            file.mtime_ms
        };
        spawn_blocking(move || disk::seal(out, mtime, &path)).await??;
        round.wrote += started.elapsed();
        if round.batch.is_empty() {
            round.oldest = Instant::now();
        }
        round.batch_bytes += file.size;
        round.batch.push(Sealed {
            temp,
            work,
            hash: file.hash,
            size: file.size,
            _fetched: fetched,
        });
        Ok(false)
    }

    /// Waits for what `ahead` gets of `hash`, putting the batch in place
    /// meanwhile if it has waited long enough.
    async fn blob_from(
        &mut self,
        ahead: &mut Ahead,
        hash: Hash,
        round: &mut Round,
    ) -> Result<Option<Fetched>> {
        let started = Instant::now();
        let got = loop {
            if let Some(got) = ahead.take(&hash) {
                break got;
            }
            let waiting = !round.batch.is_empty();
            tokio::select! {
                () = ahead.finish_one() => {}
                () = sleep_until(round.oldest + BATCH_AGE), if waiting => self.flush(round).await,
            }
        };
        round.waited += started.elapsed();
        got
    }

    /// Brings one ref in line through git, which takes its lock, writes the
    /// reflog (so a ref moved backwards keeps its old commit reachable there)
    /// and fails if the ref moved since the last scan. A ref waits until all
    /// it names is here.
    async fn bring_ref(&mut self, work: Work) -> Result<()> {
        if !self.still_wanted(&work) {
            return Ok(());
        }
        let (old, stays_ahead) = {
            let state = self.shared.lock();
            let old = state.saved.seen.get(&work.path).map(|seen| seen.hash);
            // The ref here lost to another machine's, kept beside it: the
            // settling below moves the branch forward again, if it can.
            let lost = old
                .and_then(|old| crate::files::sync::map::conflict_path(&work.path, &old))
                .and_then(|copy| state.saved.map.get(&copy).map(|e| e.file.is_some()));
            (old.map(Oid::from_bytes), lost.unwrap_or(false))
        };
        let root = self.shared.root().to_owned();
        let change = RefMove {
            path: work.path.clone(),
            new: work.file.as_ref().map(|file| Oid::from_bytes(file.hash)),
            old,
            stays_ahead,
        };
        let done = spawn_blocking(move || gitdir::move_ref(&root, &change)).await??;
        let Some(done) = done else {
            return Ok(());
        };
        let mut state = self.shared.lock();
        match work.file {
            Some(file) => state.set_seen(
                &work.path,
                Seen {
                    stat: ref_stat(file.mtime_ms),
                    hash: file.hash,
                    version: work.version,
                },
            ),
            None => {
                state.remove_seen(&work.path);
            }
        }
        state.saved.index_stale |= done == Stay::HeadMoved;
        state.unsaved = true;
        Ok(())
    }

    /// What follows a round in a repository: refs settled, and the index
    /// rebuilt if HEAD now names another commit.
    async fn finish_git(&mut self) {
        if !read_lock(&self.shared.rules).syncs_git() {
            return;
        }
        self.settle_refs().await;
        let stale = self.shared.lock().saved.index_stale;
        let root = self.shared.root().to_owned();
        let rebuilt = spawn_blocking(move || -> Result<bool> {
            let Some(repo) = Repo::open(&root) else {
                return Ok(false);
            };
            if !(stale || repo.has_no_index()) || repo.head().is_none() {
                return Ok(false);
            }
            if gitdir::is_busy(&root) {
                bail!("waiting for the git operation running on this machine");
            }
            repo.rebuild_index()?;
            exclude_from_git(&root);
            Ok(true)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|rebuilt| rebuilt);
        match rebuilt {
            Ok(true) => {
                let mut state = self.shared.lock();
                state.saved.index_stale = false;
                state.unsaved = true;
                drop(state);
                // Tracked files changed, and with them the rules.
                self.shared.touched.notify_one();
            }
            Ok(false) => {}
            Err(e) => {
                self.pending += 1;
                self.fail("rebuilding the git index failed", &e);
            }
        }
    }

    /// Looks at each ref kept for the losing side of two commits. One that
    /// the branch already holds is dropped; a branch it holds moves forward
    /// to it. Every machine reaches the same list, by ordinary edits. Commits
    /// that really diverged stay for the user.
    async fn settle_refs(&mut self) {
        let pairs: Vec<Pair> = {
            let state = self.shared.lock();
            state
                .saved
                .map
                .entries()
                .filter(|entry| gitdir::is_ref(&entry.path) && is_conflict(&entry.path))
                .filter_map(|entry| {
                    let lost = entry.file.clone()?;
                    let branch = original_of(&entry.path)?;
                    let won = state.saved.map.get(&branch)?.file.as_ref()?.hash;
                    Some(Pair {
                        copy: entry.path.clone(),
                        branch,
                        lost,
                        won,
                    })
                })
                .collect()
        };
        if pairs.is_empty() {
            return;
        }
        let root = self.shared.root().to_owned();
        let settled = spawn_blocking(move || {
            let repo = Repo::open(&root)?;
            let ahead = |a: [u8; 32], b: [u8; 32]| {
                repo.is_ancestor(Oid::from_bytes(a), Oid::from_bytes(b))
                    .unwrap_or(false)
            };
            let settled: Vec<(Pair, bool)> = pairs
                .into_iter()
                .filter_map(|pair| {
                    if ahead(pair.lost.hash, pair.won) {
                        Some((pair, false))
                    } else if ahead(pair.won, pair.lost.hash) {
                        Some((pair, true))
                    } else {
                        None
                    }
                })
                .collect();
            Some(settled)
        })
        .await;
        let settled = match settled {
            Ok(Some(settled)) if !settled.is_empty() => settled,
            Ok(_) => return,
            Err(e) => {
                log::warn!("{}: settling refs did not finish: {e}", self.shared.name);
                return;
            }
        };
        let mut state = self.shared.lock();
        for (pair, forward) in settled {
            if forward && let Some(branch) = state.saved.map.get(&pair.branch).cloned() {
                state
                    .saved
                    .map
                    .edit(&pair.branch, &branch.version, Some(pair.lost));
            }
            if let Some(copy) = state.saved.map.get(&pair.copy).cloned() {
                state.saved.map.edit(&pair.copy, &copy.version, None);
            }
        }
        state.unsaved = true;
        let seq = state.saved.map.seq();
        drop(state);
        self.shared.seq.send_replace(seq);
        self.shared.merged.notify_one();
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
        let rules = read_lock(&self.shared.rules);
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
}
