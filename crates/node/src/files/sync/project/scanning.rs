//! Scanning the folder for edits, and sending held deletes.

use super::{
    list::record,
    state::{GitCopy, Seen},
    task::{Reason, Task},
};
use crate::files::sync::{
    disk::{self, Found, OnDisk, same},
    gitdir::{self, Checkout, CheckoutPaths, Repo},
    root::{self, Checked, RootId},
    rules::{MARKER, Rules},
    scan,
    watch::{Changes, Mode},
};
use crate::{read_lock, write_lock};
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::State as SyncState;
use futures::future::join_all;
use iroh_blobs::Hash;
use std::{
    collections::{BTreeSet, HashSet},
    mem,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::task::spawn_blocking;

/// How many files a scan hashes at once.
const MAX_IMPORTS: usize = 16;
/// How long a scan runs before the status says so.
const SHOW_SCANNING: Duration = Duration::from_millis(500);

impl Task {
    /// Looks for edits on disk and records them in the list.
    pub(super) async fn scan(&mut self, reason: Reason) {
        self.scan_inner(None, reason).await;
    }

    pub(super) async fn scan_changes(&mut self, mut paths: BTreeSet<PathBuf>) {
        if self.first || self.scan_failed {
            let reason = if self.scan_failed {
                Reason::Retry
            } else {
                Reason::Changes
            };
            self.scan(reason).await;
            return;
        }

        paths.extend(self.held.iter().map(PathBuf::from));
        if !paths.is_empty() {
            self.scan_inner(Some(paths), Reason::Changes).await;
        }
    }

    /// Brings the list in line before a peer is matched: what the watcher
    /// reported is scanned, and the whole folder when the watcher cannot
    /// vouch for it or the checkout moved since the last scan. Whether
    /// anything was scanned.
    pub(super) async fn scan_for_match(&mut self, changes: Changes, touched: bool) -> bool {
        let checkout = self.shared.head().await.ok();
        let vouched = matches!(self.mode, Mode::Watching)
            && checkout.is_some()
            && self.shared.lock().saved.checkout == checkout;

        if changes.full || touched || !vouched {
            self.scan(Reason::Request).await;
            return true;
        }

        if changes.paths.is_empty() {
            return false;
        }

        self.scan_changes(changes.paths).await;
        true
    }

    async fn scan_inner(&mut self, paths: Option<BTreeSet<PathBuf>>, reason: Reason) {
        let started = Instant::now();
        metrics::counter!("beep_sync_scan_total", "reason" => Into::<&'static str>::into(reason))
            .increment(1);
        let scope = if paths.is_some() { "changed" } else { "full" };
        // A quick scan keeps the status as it is, and a failing one keeps its
        // failure shown until it recovers.
        let shared = self.shared.clone();
        let failed = self.error.is_some() || self.scan_failed;
        let result = {
            let scan = self.try_scan(paths);
            tokio::pin!(scan);

            tokio::select! {
                biased;
                result = &mut scan => result,
                () = tokio::time::sleep(SHOW_SCANNING), if !failed => {
                    shared.publish(|s| s.state = SyncState::Scanning);
                    scan.await
                }
            }
        };
        self.scan_failed = result.is_err();
        let outcome = if self.scan_failed { "error" } else { "ok" };
        metrics::counter!("beep_sync_scans_total", "scope" => scope, "outcome" => outcome)
            .increment(1);
        metrics::histogram!("beep_sync_scan_seconds", "scope" => scope, "outcome" => outcome)
            .record(started.elapsed().as_secs_f64());

        if self.scan_failed {
            // Not trusted until a scan succeeds again.
            self.held.clear();
        }

        match result {
            // Only a Git check clears a Git warning: a scan reads the disk alone.
            Ok(()) if self.waits_for_git() => {}
            Ok(()) => self.error = None,
            Err(e) => self.fail("scanning failed", &e),
        }

        self.publish_status(None);
    }

    async fn try_scan(&mut self, paths: Option<BTreeSet<PathBuf>>) -> Result<()> {
        self.shared.check_ignored().await?;
        let checked = self.check_root().await?;
        let root = self.shared.root().to_owned();
        let checkout = {
            let root = root.clone();
            spawn_blocking(move || -> Result<_> {
                if gitdir::is_busy(&root) {
                    bail!("{}", gitdir::busy_reason(&root));
                }

                Ok(Checkout::read(&root))
            })
            .await??
        };
        if paths.is_none() {
            let root = root.clone();
            let ignored = self.shared.ignored.clone();
            let extra = self.shared.rules_text.clone();
            let rules = spawn_blocking(move || match ignored {
                Some(scope) => scope.rules(&root),
                None => Rules::load(&root, &extra),
            })
            .await??;
            *write_lock(&self.shared.rules) = rules;
        }
        // A repository created locally before any donor was selected is
        // already owned by Git and must never receive a raw Git copy.
        if root.join(gitdir::GIT).exists() {
            let mut state = self.shared.lock();
            if matches!(state.saved.git_copy, Some(GitCopy::Pending { peer: None })) {
                state.saved.git_copy = Some(GitCopy::Finished);
                state.unsaved = true;
            }
        }

        if checkout.present {
            let mut state = self.shared.lock();
            state.saved.git_only = true;
            state.unsaved = true;
        }

        let syncs_git = read_lock(&self.shared.rules).syncs_git();
        // Before the walk: git writes objects before the refs that name
        // them, so the walk finds every object these refs need.
        let repo = {
            let root = root.clone();
            spawn_blocking(move || {
                Repo::open(&root)
                    .filter(|_| syncs_git)
                    .map(|repo| repo.refs())
            })
            .await?
        };
        // Refs git cannot read leave the files to sync all the same: the
        // repository is then treated as not here, so none of its files is
        // deleted and no ref is sent.
        let repo = match repo {
            Some(Err(e)) => {
                log::warn!(
                    "{}: the refs cannot be read, so they wait for the next scan: {e:#}",
                    self.shared.name
                );
                None
            }
            Some(Ok(refs)) => Some(refs),
            None => None,
        };
        // No repository to read: its files missing are not deletes.
        let repo_gone = repo.is_none();
        let refs = repo.unwrap_or_default();
        let git_only = self.shared.lock().saved.git_only;
        let seen = paths
            .as_ref()
            .map(|_| self.shared.lock().saved.seen.clone());
        let previous_dirs = self.dirs.clone();
        let (mut walked, protected) = {
            let (root, rules) = (root.clone(), self.shared.rules.clone());
            let cache = self.shared.file.with_extension("seed");
            let protect_recovery = self.shared.recovery_enabled();
            let selected = paths.clone();
            spawn_blocking(move || -> Result<_> {
                let rules = read_lock(&rules);
                if git_only {
                    let repo = Repo::open(&root).filter(|repo| {
                        selected.is_some() || protect_recovery || repo.has_recovery_backups()
                    });
                    let protected = repo.is_some();
                    let mut walked = match &selected {
                        Some(selected) => {
                            scan::walk_changed(&root, &rules, selected, true, protected)?
                        }
                        _ => scan::walk_git(&root, &rules, protected)?,
                    };

                    if let Some(repo) = repo {
                        walked
                            .files
                            .extend(repo.seed_objects(&cache, &repo.refs()?)?);
                    }

                    walked.check_limits()?;
                    Ok((walked, protected))
                } else {
                    Ok((
                        match &selected {
                            Some(selected) => {
                                scan::walk_changed(&root, &rules, selected, false, false)?
                            }
                            None => scan::walk(&root, &rules, 0)?,
                        },
                        false,
                    ))
                }
            })
            .await??
        };
        metrics::counter!("beep_sync_inspected_files_total").increment(walked.files.len() as u64);
        if let (Some(paths), Some(seen)) = (&paths, seen) {
            // Cached metadata supplies unchanged entries. Only event paths touched the disk.
            let refreshed: HashSet<_> = walked.files.iter().map(|file| file.path.clone()).collect();
            for (path, prior) in seen {
                if refreshed.contains(&path)
                    || gitdir::is_ref(&path)
                    || paths
                        .iter()
                        .any(|changed| Path::new(&path).starts_with(changed))
                    || protected && path.starts_with(".git/objects/")
                {
                    continue;
                }
                walked.files.push(Found {
                    abs: root.join(&path),
                    path,
                    stat: prior.stat,
                });
            }
            walked.dirs.extend(
                previous_dirs
                    .into_iter()
                    .filter(|dir| !paths.iter().any(|path| dir.starts_with(root.join(path)))),
            );
            walked.dirs.sort();
            walked.dirs.dedup();
            walked.check_limits()?;
        }
        *write_lock(&self.shared.bootstrap_paths) = protected.then(|| {
            walked
                .files
                .iter()
                .chain(refs.iter().map(|r| &r.found))
                .map(|f| f.path.clone())
                .collect()
        });

        if let Some(scope) = &self.shared.ignored {
            walked.dirs.extend(scope.watch_dirs());
            walked.dirs.sort();
            walked.dirs.dedup();
        }

        let dirs: HashSet<_> = walked.dirs.iter().cloned().collect();
        if self.dirs != dirs || !matches!(self.mode, crate::files::sync::watch::Mode::Watching) {
            self.mode = self
                .watcher
                .set_dirs(self.shared.key, walked.dirs.clone())
                .await;
        }
        // A file made in a new folder after the walk read it and before its
        // watch was set shows in nothing but another walk.
        if walked
            .dirs
            .iter()
            .any(|dir| dir != &root && !self.dirs.contains(dir))
        {
            self.watcher.recheck(
                self.shared.key,
                walked
                    .dirs
                    .iter()
                    .filter(|dir| *dir != &root && !self.dirs.contains(*dir))
                    .filter_map(|dir| dir.strip_prefix(&root).ok().map(Path::to_owned)),
            );
        }

        self.dirs = dirs;
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
        let mut changed = Vec::new();

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

            changed.push((found, prior));
        }

        for batch in changed.chunks(MAX_IMPORTS) {
            if self.shared.stop.is_cancelled() {
                bail!("stopped");
            }

            let cache = self.shared.file.with_extension("seed");
            let imports = batch.iter().map(|(found, _)| async {
                if found.abs.starts_with(&cache) {
                    scan::import_copy(&self.parts.store, found).await
                } else {
                    scan::import(&self.parts.store, found).await
                }
            });

            for ((found, prior), imported) in batch.iter().zip(join_all(imports).await) {
                match imported {
                    Ok(tag) => {
                        hashed.push((*found, *tag.hash().as_bytes(), prior.clone()));
                        tags.push(tag);
                    }
                    // Changed while it was hashed: the next scan sees it again.
                    Err(e) => log::debug!("{}: {e:#}", self.shared.name),
                }
            }
        }
        {
            let state = self.shared.lock();

            for now in &refs {
                let prior = state.saved.seen.get(&now.found.path).cloned();

                if prior
                    .as_ref()
                    .is_none_or(|prior| prior.hash != *now.oid.as_ref())
                {
                    hashed.push((&now.found, *now.oid.as_ref(), prior));
                }
            }
        }
        self.shared.check_ignored().await?;
        // HEAD last: a peer applying part of the list finds what it names.
        hashed.sort_by_key(|(found, ..)| gitdir::kind(&found.path) == Some(gitdir::Kind::Head));
        let first = self.first;
        let kept: HashSet<&str> = walked
            .files
            .iter()
            .chain(refs.iter().map(|now| &now.found))
            .map(|f| f.path.as_str())
            .collect();
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
                        let missing = if gitdir::in_git(&path) && repo_gone {
                            false
                        } else if gitdir::is_ref(&path) {
                            true
                        } else {
                            matches!(disk::look(&root, &path), Ok(OnDisk::Missing))
                        };
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
        let git_advanced = self
            .shared
            .lock()
            .saved
            .fast_forward
            .is_some_and(|target| checkout.commit == Some(target));
        let checkout_paths = if git_advanced {
            let root = root.clone();
            spawn_blocking(move || {
                Repo::open(&root)
                    .context("the Git checkout disappeared")?
                    .checkout_paths()
            })
            .await??
        } else {
            CheckoutPaths::default()
        };
        let stable = {
            let root = root.clone();
            spawn_blocking(move || !gitdir::is_busy(&root) && Checkout::read(&root) == checkout)
                .await?
        };

        if !stable {
            bail!("the Git checkout changed during the scan; retrying sync");
        }

        let (before, seq, checkout_changed) = {
            let mut state = self.shared.lock();
            let before = state.saved.map.seq();
            let checkout_changed = state.saved.checkout != Some(checkout);

            for (found, hash, prior) in hashed {
                record(
                    &mut state,
                    found,
                    hash,
                    prior,
                    checkout_paths.clean.contains(&found.path),
                );
            }

            for (path, missing) in deleted {
                // Kept as seen, and sent by `send_deletes` once the
                // folder settled and is still the one synced.
                if missing && baseline {
                    if git_advanced
                        && !gitdir::in_git(&path)
                        && !checkout_paths.changed.contains(&path)
                    {
                        if let Some(seen) = state.remove_seen(&path) {
                            state.saved.map.baseline(&path, seen.version, None);
                        }
                    } else {
                        held.insert(path);
                    }
                } else {
                    state.remove_seen(&path);
                }
            }

            state.saved.checkout = Some(checkout);
            state.saved.fast_forward = None;
            state.unsaved = true;
            (before, state.saved.map.seq(), checkout_changed)
        };
        self.held = held;
        self.first = false;

        if seq != before || checkout_changed {
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
    pub(super) async fn send_deletes(&mut self) {
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
        self.shared.check_ignored().await?;
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
                    .filter(|path| {
                        gitdir::is_ref(path)
                            || matches!(disk::look(&root, path), Ok(OnDisk::Missing))
                    })
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
    pub(super) async fn check_root(&self) -> Result<Checked> {
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
}

/// Whether the files a scan found missing are deletes. On the first scan
/// after a start, a folder that holds none of the files seen before was
/// emptied or replaced while sync was off: what it misses is fetched
/// again rather than deleted on every other machine.
pub(super) fn has_baseline(first: bool, seen: usize, gone: &[(String, bool)]) -> bool {
    let all_missing = gone.len() == seen && gone.iter().all(|(_, missing)| *missing);
    !(first && seen > 0 && all_missing)
}
