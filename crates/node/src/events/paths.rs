//! Filesystem watches that follow missing directories without watching whole ancestors recursively.
//!
//! On Linux a recursive watch is one inotify watch per folder, so the walk
//! leaves out `UNWATCHED`: on a large checkout its build output alone took
//! longer to watch than a reader waits.

use anyhow::{Context, Result, bail};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::mpsc, task::spawn_blocking};
use tokio_util::task::AbortOnDropHandle;

type Changed = Arc<dyn Fn(Vec<PathBuf>, bool) + Send + Sync>;

pub(crate) struct Paths {
    inner: Arc<Mutex<Inner>>,
    _reconcile: AbortOnDropHandle<()>,
    changed: Changed,
    #[cfg(test)]
    reconciled: tokio::sync::watch::Receiver<()>,
}

struct Inner {
    watcher: RecommendedWatcher,
    stale: Arc<AtomicBool>,
    roots: Arc<Mutex<Vec<PathBuf>>>,
    installed: BTreeMap<PathBuf, RecursiveMode>,
    /// The folders each recursive watch holds, one inotify watch each.
    #[cfg(target_os = "linux")]
    folders: BTreeMap<PathBuf, std::collections::BTreeSet<PathBuf>>,
    /// Paths created, removed or renamed since the folders were last
    /// brought in line, in the order they changed.
    #[cfg(target_os = "linux")]
    moved: Arc<Mutex<Vec<PathBuf>>>,
}

/// Folders a watch never walks into: linked worktrees, build output and
/// caches. Their changes are dropped anyway. A root inside one, such as a
/// worktree a session works in, is watched as a root of its own.
const UNWATCHED: &[&str] = &[
    ".worktrees",
    "target",
    "node_modules",
    ".gradle",
    ".cxx",
    ".build",
    "DerivedData",
    "__pycache__",
    ".venv",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
];

impl Paths {
    pub(crate) fn new(changed: impl Fn(Vec<PathBuf>) + Send + Sync + 'static) -> Result<Self> {
        Self::with_structure(move |roots, _| changed(roots))
    }

    pub(crate) fn with_structure(
        changed: impl Fn(Vec<PathBuf>, bool) + Send + Sync + 'static,
    ) -> Result<Self> {
        let roots = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let interested = roots.clone();
        let stale = Arc::new(AtomicBool::new(false));
        let invalidated = stale.clone();
        #[cfg(target_os = "linux")]
        let moved = Arc::new(Mutex::new(Vec::new()));
        #[cfg(target_os = "linux")]
        let followed = moved.clone();
        let (refresh, mut refreshes) = mpsc::channel(1);
        let changed: Changed = Arc::new(changed);
        let reconciled_callback = changed.clone();
        let direct = changed.clone();
        #[cfg(test)]
        let (reconciled_tx, reconciled) = tokio::sync::watch::channel(());
        let watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                counted("paths", &event);
                let roots = crate::lock(&interested);
                let changed_roots = match &event {
                    Ok(event) if event.need_rescan() => roots.clone(),
                    Ok(event) if matches!(event.kind, EventKind::Access(_) | EventKind::Other) => {
                        return;
                    }
                    Ok(event) => roots
                        .iter()
                        .filter(|root| event.paths.iter().any(|path| affects(root, path)))
                        .cloned()
                        .collect::<Vec<_>>(),
                    Err(_) => roots.clone(),
                };
                let renames = |event: &notify::Event| {
                    matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Remove(_)
                            | EventKind::Modify(notify::event::ModifyKind::Name(_))
                    )
                };

                if event.as_ref().map_or(true, |event| {
                    event.need_rescan()
                        || (renames(event) && event.paths.iter().any(|path| loses_watch(&roots, path)))
                }) {
                    invalidated.store(true, Ordering::Release);
                }
                drop(roots);

                // Folders follow every change in a watched folder, also one
                // no root reports, such as Git's own: notify cannot be changed
                // from its own callback, so the reconcile task walks them.
                #[cfg(target_os = "linux")]
                if let Ok(event) = &event
                    && renames(event)
                {
                    crate::lock(&followed).extend(event.paths.iter().cloned());
                    let _ = refresh.try_send(());
                }

                if changed_roots.is_empty() {
                    return;
                }

                let structural = event
                    .as_ref()
                    .map_or(true, |event| event.need_rescan() || renames(event));
                direct(changed_roots, structural);

                if structural {
                    let _ = refresh.try_send(());
                }
            },
            notify::Config::default().with_follow_symlinks(false),
        )?;
        let inner = Arc::new(Mutex::new(Inner {
            watcher,
            stale,
            roots,
            installed: BTreeMap::new(),
            #[cfg(target_os = "linux")]
            folders: BTreeMap::new(),
            #[cfg(target_os = "linux")]
            moved,
        }));
        let weak = Arc::downgrade(&inner);
        let reconcile = AbortOnDropHandle::new(tokio::spawn(async move {
            while refreshes.recv().await.is_some() {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let (result, roots) = match spawn_blocking(move || {
                    let mut held = crate::lock(&inner);
                    let roots = crate::lock(&held.roots).clone();
                    let result = held.install(&roots);
                    #[cfg(target_os = "linux")]
                    held.follow();
                    (result, roots)
                })
                .await
                {
                    Ok(done) => done,
                    Err(_) => return,
                };

                if result.is_err() {
                    log::debug!("filesystem notification watches need reconciliation");
                }
                // Rebuilding an OS watch can leave a short gap; reconcile state afterwards.
                if !matches!(result, Ok(false)) {
                    reconciled_callback(roots, true);
                }
                #[cfg(test)]
                reconciled_tx.send_replace(());
            }
        }));
        Ok(Self {
            inner,
            _reconcile: reconcile,
            changed,
            #[cfg(test)]
            reconciled,
        })
    }

    pub(crate) async fn replace(&self, roots: Vec<PathBuf>) -> Result<()> {
        let inner = self.inner.clone();
        let (result, changed) = spawn_blocking(move || {
            // OS notifications use physical paths, including macOS's /private aliases.
            // Resolve the existing prefix even when a requested store is not created yet.
            let mut roots = roots
                .iter()
                .map(|root| crate::agents::reach::real(root))
                .collect::<Result<Vec<_>>>()?;
            roots.sort();
            roots.dedup();
            let mut inner = crate::lock(&inner);
            let before = crate::lock(&inner.roots).clone();
            let result = inner.install(&roots);

            if result.is_ok() {
                *crate::lock(&inner.roots) = roots.clone();
            }

            let changed = if before != roots || !matches!(result, Ok(false)) {
                before.into_iter().chain(roots).collect()
            } else {
                Vec::new()
            };
            Ok::<_, anyhow::Error>((result, changed))
        })
        .await
        .context("filesystem watcher stopped")??;

        if !changed.is_empty() {
            (self.changed)(changed, true);
        }

        result.map(|_| ())
    }
}

impl Inner {
    fn install(&mut self, roots: &[PathBuf]) -> Result<bool> {
        let mut wanted = BTreeMap::new();

        for root in roots {
            let (path, mode) = existing(root)?;
            wanted
                .entry(path)
                .and_modify(|held| {
                    if mode == RecursiveMode::Recursive {
                        *held = mode;
                    }
                })
                .or_insert(mode);
        }
        // A recursive parent already covers its descendants.
        let recursive: Vec<_> = wanted
            .iter()
            .filter(|(_, mode)| **mode == RecursiveMode::Recursive)
            .map(|(path, _)| path.clone())
            .collect();
        wanted.retain(|path, _| !recursive.iter().any(|parent| covers(parent, path)));
        let stale = self.stale.swap(false, Ordering::AcqRel);

        if self.installed == wanted && !stale {
            return Ok(false);
        }
        // notify owns OS watch descriptors by path, without caller reference counts.
        // Remove the previous tree before rebuilding overlapping roots.
        for path in self.installed.clone().keys() {
            self.unwatch(path);
        }

        let mut added: Vec<PathBuf> = Vec::new();

        for (path, mode) in &wanted {
            if let Err(error) = self.watch(path, *mode) {
                self.unwatch(path);

                for path in added {
                    self.unwatch(&path);
                }

                for (path, mode) in self.installed.clone() {
                    let _ = self.watch(&path, mode);
                }

                self.stale.store(true, Ordering::Release);
                return Err(error.into());
            }

            added.push(path.clone());
        }

        self.installed = wanted;
        Ok(true)
    }

    /// Watches one root. On Linux a folder is walked here, one watch per
    /// folder outside `UNWATCHED`, instead of by notify, which walks all.
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        #[cfg(target_os = "linux")]
        if mode == RecursiveMode::Recursive && path.is_dir() {
            let held = self.folders.entry(path.to_owned()).or_default();
            return watch_folders(&mut self.watcher, path, held);
        }

        self.watcher.watch(path, mode)
    }

    fn unwatch(&mut self, path: &Path) {
        #[cfg(target_os = "linux")]
        if let Some(folders) = self.folders.remove(path) {
            for folder in folders {
                let _ = self.watcher.unwatch(&folder);
            }
            return;
        }

        let _ = self.watcher.unwatch(path);
    }

    /// Brings the folder watches in line with the paths that moved: a folder
    /// that appeared is walked and watched, and one that went is let go.
    /// Paths are taken in the order they changed, so a folder renamed away
    /// is let go before its new name takes the same inotify watch.
    #[cfg(target_os = "linux")]
    fn follow(&mut self) {
        let moved = std::mem::take(&mut *crate::lock(&self.moved));

        for path in moved {
            let Some(held) = self
                .folders
                .iter_mut()
                .filter(|(root, _)| covers(root, &path))
                .max_by_key(|(root, _)| root.as_os_str().len())
                .map(|(_, held)| held)
            else {
                continue;
            };

            if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir()) {
                if !held.contains(&path) {
                    let _ = watch_folders(&mut self.watcher, &path, held);
                }
                continue;
            }

            let gone: Vec<_> = held
                .range(path.clone()..)
                .take_while(|folder| folder.starts_with(&path))
                .cloned()
                .collect();

            for folder in gone {
                held.remove(&folder);
                let _ = self.watcher.unwatch(&folder);
            }
        }
    }
}

/// Watches `start` and every folder under it outside `UNWATCHED`, adding
/// each to `held`. A folder under `start` that goes away or cannot be read
/// during the walk is left out; running out of inotify watches is an error.
#[cfg(target_os = "linux")]
fn watch_folders(
    watcher: &mut RecommendedWatcher,
    start: &Path,
    held: &mut std::collections::BTreeSet<PathBuf>,
) -> notify::Result<()> {
    let mut pending = vec![start.to_owned()];

    while let Some(folder) = pending.pop() {
        match watcher.watch(&folder, RecursiveMode::NonRecursive) {
            Ok(()) => {}
            Err(error)
                if folder != start && !matches!(error.kind, notify::ErrorKind::MaxFilesWatch) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
        held.insert(folder.clone());

        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        pending.extend(
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter(|entry| {
                    !entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| UNWATCHED.contains(&name))
                })
                .map(|entry| entry.path()),
        );
    }

    Ok(())
}

/// Counts one filesystem notification by the watcher that took it.
pub(crate) fn counted(watcher: &'static str, event: &notify::Result<notify::Event>) {
    let kind = match event {
        Ok(event) if event.need_rescan() => "rescan",
        Ok(event) => match event.kind {
            EventKind::Create(_) => "create",
            EventKind::Modify(_) => "modify",
            EventKind::Remove(_) => "remove",
            EventKind::Access(_) => "access",
            EventKind::Any | EventKind::Other => "other",
        },
        Err(_) => "error",
    };
    metrics::counter!("beep_watch_events_total", "watcher" => watcher, "kind" => kind)
        .increment(1);
}

/// Git's own folder is left out of a checkout's root: objects, logs and
/// fetch results change on every Git command. Callers that need its refs or
/// index watch those paths as roots of their own.
fn affects(root: &Path, path: &Path) -> bool {
    if let Ok(relative) = path.strip_prefix(root) {
        return !unwatched(relative)
            && !relative
                .components()
                .any(|component| component.as_os_str() == ".git");
    }

    root.starts_with(path)
}

/// Whether a recursive watch of `parent` holds the watch for `path` below
/// it. On Linux it does not reach into `UNWATCHED` folders.
fn covers(parent: &Path, path: &Path) -> bool {
    if cfg!(target_os = "linux") {
        return reports(parent, path);
    }

    path != parent && path.starts_with(parent)
}

/// Whether changes at `path`, below `parent`, reach a watch of `parent`:
/// not those inside an `UNWATCHED` folder, such as a linked worktree.
pub(crate) fn reports(parent: &Path, path: &Path) -> bool {
    path != parent
        && path
            .strip_prefix(parent)
            .is_ok_and(|relative| !unwatched(relative))
}

/// Whether a relative path passes through an `UNWATCHED` folder.
fn unwatched(relative: &Path) -> bool {
    relative.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| UNWATCHED.contains(&name))
    })
}

/// Whether replacing `path` drops a watch: it is, or holds, a root no other
/// root outside it covers. Git replaces a checkout's `.git/index` on most
/// commands, and the recursive watch of the checkout still covers it, so that
/// alone does not rebuild every watch and read every root again.
fn loses_watch(roots: &[PathBuf], path: &Path) -> bool {
    roots.iter().any(|root| {
        root.starts_with(path)
            && !roots
                .iter()
                .any(|other| covers(other, root) && !other.starts_with(path))
    })
}

fn existing(root: &Path) -> Result<(PathBuf, RecursiveMode)> {
    if root.exists() {
        return Ok((root.to_owned(), RecursiveMode::Recursive));
    }

    for parent in root.ancestors().skip(1) {
        if parent.is_dir() {
            return Ok((parent.to_owned(), RecursiveMode::NonRecursive));
        }
    }

    bail!("the watched path has no existing parent")
}

#[cfg(test)]
#[path = "../tests/events_paths.rs"]
mod tests;
