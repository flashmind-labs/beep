//! Bringing the folder in line with the list.

use super::{
    fetch::copy_out,
    list::{Work, move_target, plan},
    state::Seen,
    task::Task,
};
use crate::files::sync::{
    disk::{self},
    progress::{self, Meter},
    root::Checked,
    rules::MARKER,
    scan::{self, Found, Stat},
};
use crate::files::transfer::place::Partial;
use anyhow::{Result, anyhow, bail};
use beep_proto::node::sync::{MAX_WAITING, State as SyncState};
use iroh_blobs::Hash;
use std::{ffi::OsString, sync::PoisonError, time::Duration};
use tokio::{fs::File as AsyncFile, io::AsyncWriteExt, task::spawn_blocking, time::Instant};

/// How often progress is published while files are brought in line.
const SHOW_EVERY: Duration = Duration::from_millis(500);

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
}
