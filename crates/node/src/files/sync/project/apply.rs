//! Bringing the folder in line with the list.

use super::{
    fetch::copy_out,
    list::{Work, move_target, plan},
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
use crate::{files::transfer::place::Partial, read_lock};
use anyhow::{Result, anyhow, bail};
use beep_proto::node::sync::{MAX_WAITING, State as SyncState};
use iroh_blobs::Hash;
use std::{
    ffi::OsString,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{fs::File as AsyncFile, io::AsyncWriteExt, task::spawn_blocking, time::Instant};

/// How often progress is published while files are brought in line.
const SHOW_EVERY: Duration = Duration::from_millis(500);

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
        if work.is_empty() {
            self.pending = 0;
            self.waiting.clear();
            self.finish_git().await;
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
            // HEAD moves only once all it may name is here.
            let brought = if failed > 0 && gitdir::kind(&path) == Some(Kind::Head) {
                Err(anyhow!("waiting for the rest of the repository"))
            } else {
                self.bring(item, &mut partial).await
            };
            match brought {
                Ok(()) => meter.done(size, Instant::now()),
                Err(e) => {
                    // The first failure says why; what follows may only
                    // be waiting for it.
                    if failed == 0 {
                        self.fail("syncing a file failed", &e);
                    } else {
                        log::debug!("{}: {path}: {e:#}", self.shared.name);
                    }
                    failed += 1;
                    meter.failed(size);
                    if lost.len() < MAX_WAITING {
                        lost.push(path);
                    }
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
        self.finish_git().await;
        if let Err(e) = self.protect().await {
            self.fail("keeping the files in the store failed", &e);
        }
        self.publish_status(None);
    }

    /// Brings one path in line, unless the list or the disk moved on since
    /// it was planned.
    async fn bring(&mut self, work: Work, partial: &mut Option<Partial>) -> Result<()> {
        if gitdir::is_ref(&work.path) {
            return self.bring_ref(work).await;
        }
        let kind = gitdir::kind(&work.path);
        if kind == Some(Kind::Head) {
            let root = self.shared.root().to_owned();
            if spawn_blocking(move || gitdir::is_busy(&root)).await? {
                bail!("waiting for the git operation running on this machine");
            }
        }
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
                self.copy_from_peers(hash, out).await
            },
        )
        .await?;
        out.flush().await?;
        let out = out.into_std().await;
        let stat = {
            let (root, dir) = (self.shared.root.clone(), partial.dir().try_clone()?);
            // An object must not look old enough for this machine's next
            // `git gc` to prune it before the ref that names it comes.
            let mtime = if kind == Some(Kind::Object) {
                now_ms()
            } else {
                file.mtime_ms
            };
            let (path, expected) = (work.path.clone(), work.expected);
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
        let mut state = self.shared.lock();
        state.set_seen(
            &work.path,
            Seen {
                stat,
                hash: file.hash,
                version: work.version,
            },
        );
        state.saved.index_stale |= kind == Some(Kind::Head);
        Ok(())
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
}
