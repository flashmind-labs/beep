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
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    time::{Duration, Instant},
};
use tokio::task::spawn_blocking;

type Changed = Arc<dyn Fn(Vec<PathBuf>, bool) + Send + Sync>;

/// How long a root stays watched after its last reader lets go, so a view
/// opened again does not walk the folder again.
const LINGER: Duration = Duration::from_secs(5 * 60);

/// One reader of the node's filesystem watch: it names the roots it wants
/// and hears about changes under them. A root is watched once, however many
/// readers want it.
pub(crate) struct Paths {
    id: u64,
    shared: Arc<Shared>,
    changed: Changed,
    #[cfg(test)]
    reconciled: tokio::sync::watch::Receiver<()>,
}

/// The node's one watcher. Its watches are brought in line on a thread of
/// its own, so it serves every runtime in the process.
struct Shared {
    inner: Mutex<Inner>,
    state: Arc<State>,
    #[cfg(test)]
    reconciled: tokio::sync::watch::Sender<()>,
}

/// What the watcher's callback shares with the rest. The callback never
/// takes `Inner`: notify waits on it while a watch is added.
struct State {
    readers: Mutex<Readers>,
    stale: AtomicBool,
    /// Paths created, removed or renamed since the folders were last
    /// brought in line, in the order they changed.
    #[cfg(target_os = "linux")]
    moved: Mutex<Vec<PathBuf>>,
    refresh: SyncSender<()>,
}

#[derive(Default)]
struct Readers {
    held: BTreeMap<u64, Reader>,
    /// Roots no reader wants any more, until they stop being watched.
    lingering: BTreeMap<PathBuf, Instant>,
}

struct Reader {
    roots: Vec<PathBuf>,
    changed: Changed,
}

struct Inner {
    watcher: RecommendedWatcher,
    state: Arc<State>,
    installed: BTreeMap<PathBuf, RecursiveMode>,
    /// The folders each recursive watch holds, one inotify watch each.
    #[cfg(target_os = "linux")]
    folders: BTreeMap<PathBuf, std::collections::BTreeSet<PathBuf>>,
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

static SHARED: Mutex<Option<Arc<Shared>>> = Mutex::new(None);
static NEXT_READER: AtomicU64 = AtomicU64::new(0);

impl Paths {
    pub(crate) fn new(changed: impl Fn(Vec<PathBuf>) + Send + Sync + 'static) -> Result<Self> {
        Self::with_structure(move |roots, _| changed(roots))
    }

    pub(crate) fn with_structure(
        changed: impl Fn(Vec<PathBuf>, bool) + Send + Sync + 'static,
    ) -> Result<Self> {
        let shared = Shared::get()?;
        let id = NEXT_READER.fetch_add(1, Ordering::Relaxed);
        let changed: Changed = Arc::new(changed);
        crate::lock(&shared.state.readers).held.insert(
            id,
            Reader {
                roots: Vec::new(),
                changed: changed.clone(),
            },
        );

        Ok(Self {
            id,
            #[cfg(test)]
            reconciled: shared.reconciled.subscribe(),
            shared,
            changed,
        })
    }

    pub(crate) async fn replace(&self, roots: Vec<PathBuf>) -> Result<()> {
        let shared = self.shared.clone();
        let id = self.id;
        let (result, changed) = spawn_blocking(move || {
            // OS notifications use physical paths, including macOS's /private aliases.
            // Resolve the existing prefix even when a requested store is not created yet.
            let mut roots = roots
                .iter()
                .map(|root| crate::agents::reach::real(root))
                .collect::<Result<Vec<_>>>()?;
            roots.sort();
            roots.dedup();

            for root in &roots {
                existing(root)?;
            }

            let mut inner = crate::lock(&shared.inner);
            let before = shared.state.set(id, roots.clone());
            let result = inner.install(&shared.state.wanted());

            if result.is_err() {
                shared.state.set(id, before.clone());
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

impl Drop for Paths {
    fn drop(&mut self) {
        let mut readers = crate::lock(&self.shared.state.readers);

        if let Some(reader) = readers.held.remove(&self.id) {
            readers.linger(reader.roots);
        }
        drop(readers);

        let _ = self.shared.state.refresh.try_send(());
    }
}

impl Shared {
    /// The node's watcher, started by its first reader.
    fn get() -> Result<Arc<Self>> {
        let mut held = crate::lock(&SHARED);

        if let Some(shared) = held.as_ref() {
            return Ok(shared.clone());
        }

        let (refresh, refreshes) = mpsc::sync_channel(1);
        let state = Arc::new(State {
            readers: Mutex::default(),
            stale: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            moved: Mutex::default(),
            refresh,
        });
        let notified = state.clone();
        let watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| notified.notify(event),
            notify::Config::default().with_follow_symlinks(false),
        )?;
        let shared = Arc::new(Self {
            inner: Mutex::new(Inner {
                watcher,
                state: state.clone(),
                installed: BTreeMap::new(),
                #[cfg(target_os = "linux")]
                folders: BTreeMap::new(),
            }),
            state,
            #[cfg(test)]
            reconciled: tokio::sync::watch::Sender::new(()),
        });
        let reconciling = shared.clone();
        std::thread::Builder::new()
            .name("beep-paths".into())
            .spawn(move || reconciling.reconcile(refreshes))?;
        *held = Some(shared.clone());
        Ok(shared)
    }

    /// Brings the watches in line whenever the callback or a reader asks,
    /// and lets lingering roots go once their time is up.
    fn reconcile(&self, refreshes: mpsc::Receiver<()>) {
        loop {
            let next = crate::lock(&self.state.readers)
                .lingering
                .values()
                .min()
                .copied();
            let wait = next.map_or(Duration::MAX, |until| {
                until.saturating_duration_since(Instant::now())
            });

            if let Err(RecvTimeoutError::Disconnected) = refreshes.recv_timeout(wait) {
                return;
            }

            let now = Instant::now();
            crate::lock(&self.state.readers)
                .lingering
                .retain(|_, until| *until > now);
            let mut inner = crate::lock(&self.inner);
            let result = inner.install(&self.state.wanted());
            #[cfg(target_os = "linux")]
            inner.follow();
            drop(inner);

            if result.is_err() {
                log::debug!("filesystem notification watches need reconciliation");
            }
            // Rebuilding an OS watch can leave a short gap; reconcile state afterwards.
            if !matches!(result, Ok(false)) {
                for (changed, roots) in self.state.readers_roots() {
                    changed(roots, true);
                }
            }
            #[cfg(test)]
            self.reconciled.send_replace(());
        }
    }
}

impl State {
    /// Hands one notification to the readers whose roots it touches.
    fn notify(&self, event: notify::Result<notify::Event>) {
        counted("paths", &event);

        if let Ok(event) = &event
            && matches!(event.kind, EventKind::Access(_) | EventKind::Other)
            && !event.need_rescan()
        {
            return;
        }

        let renames = |event: &notify::Event| {
            matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(notify::event::ModifyKind::Name(_))
            )
        };
        let structural = event
            .as_ref()
            .map_or(true, |event| event.need_rescan() || renames(event));
        let readers = crate::lock(&self.readers);
        let reached: Vec<_> = readers
            .held
            .values()
            .map(|reader| {
                let roots = match &event {
                    Ok(event) if !event.need_rescan() => reader
                        .roots
                        .iter()
                        .filter(|root| event.paths.iter().any(|path| affects(root, path)))
                        .cloned()
                        .collect(),
                    _ => reader.roots.clone(),
                };
                (reader.changed.clone(), roots)
            })
            .filter(|(_, roots): &(Changed, Vec<PathBuf>)| !roots.is_empty())
            .collect();

        if event.as_ref().map_or(true, |event| {
            event.need_rescan()
                || (renames(event) && {
                    let roots = readers.roots();
                    event.paths.iter().any(|path| loses_watch(&roots, path))
                })
        }) {
            self.stale.store(true, Ordering::Release);
        }
        drop(readers);

        // Folders follow every change in a watched folder, also one no root
        // reports, such as Git's own: notify cannot be changed from its own
        // callback, so the reconcile thread walks them.
        #[cfg(target_os = "linux")]
        if let Ok(event) = &event
            && renames(event)
        {
            crate::lock(&self.moved).extend(event.paths.iter().cloned());
            let _ = self.refresh.try_send(());
        }

        if reached.is_empty() {
            return;
        }

        for (changed, roots) in reached {
            changed(roots, structural);
        }

        if structural {
            let _ = self.refresh.try_send(());
        }
    }

    /// Gives reader `id` its `roots`, and the ones it held before.
    fn set(&self, id: u64, roots: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut readers = crate::lock(&self.readers);
        let before = readers
            .held
            .get_mut(&id)
            .map(|reader| std::mem::replace(&mut reader.roots, roots.clone()))
            .unwrap_or_default();

        for root in &roots {
            readers.lingering.remove(root);
        }
        readers.linger(before.clone());
        before
    }

    /// Every root a reader wants or that still lingers.
    fn wanted(&self) -> Vec<PathBuf> {
        let readers = crate::lock(&self.readers);
        let mut roots = readers.roots();
        roots.extend(readers.lingering.keys().cloned());
        roots.sort();
        roots.dedup();
        roots
    }

    fn readers_roots(&self) -> Vec<(Changed, Vec<PathBuf>)> {
        crate::lock(&self.readers)
            .held
            .values()
            .filter(|reader| !reader.roots.is_empty())
            .map(|reader| (reader.changed.clone(), reader.roots.clone()))
            .collect()
    }
}

impl Readers {
    fn roots(&self) -> Vec<PathBuf> {
        self.held
            .values()
            .flat_map(|reader| reader.roots.iter().cloned())
            .collect()
    }

    /// Keeps `roots` watched for `LINGER` once no reader wants them.
    fn linger(&mut self, roots: Vec<PathBuf>) {
        let wanted = self.roots();
        let until = Instant::now() + LINGER;

        for root in roots {
            if !wanted.contains(&root) {
                self.lingering.insert(root, until);
            }
        }
    }
}

impl Inner {
    fn install(&mut self, roots: &[PathBuf]) -> Result<bool> {
        let mut wanted = BTreeMap::new();

        for (path, mode) in roots.iter().filter_map(|root| existing(root).ok()) {
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
        let stale = self.state.stale.swap(false, Ordering::AcqRel);

        if self.installed == wanted && !stale {
            return Ok(false);
        }
        // notify owns OS watch descriptors by path, without caller reference
        // counts: watches that go are removed before overlapping ones are
        // added. Only what changed is walked again, unless a watch was lost.
        let (gone, added): (Vec<_>, Vec<_>) = if stale {
            (
                self.installed.clone().into_iter().collect(),
                wanted.clone().into_iter().collect(),
            )
        } else {
            (
                self.installed
                    .iter()
                    .filter(|(path, mode)| wanted.get(*path) != Some(*mode))
                    .map(|(path, mode)| (path.clone(), *mode))
                    .collect(),
                wanted
                    .iter()
                    .filter(|(path, mode)| self.installed.get(*path) != Some(*mode))
                    .map(|(path, mode)| (path.clone(), *mode))
                    .collect(),
            )
        };

        for (path, _) in &gone {
            self.unwatch(path);
        }

        for (index, (path, mode)) in added.iter().enumerate() {
            match self.watch(path, *mode) {
                Ok(()) => {}
                Err(error) if matches!(error.kind, notify::ErrorKind::MaxFilesWatch) => {
                    for (path, _) in &added[..=index] {
                        self.unwatch(path);
                    }

                    for (path, mode) in &gone {
                        let _ = self.watch(path, *mode);
                    }

                    self.state.stale.store(true, Ordering::Release);
                    return Err(error.into());
                }
                // A root removed since it was looked at, such as a deleted
                // worktree, fails alone; its removal brings another pass.
                Err(error) => {
                    log::debug!("not watching {}: {error}", path.display());
                    self.unwatch(path);
                    wanted.remove(path);
                }
            }
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
        let moved = std::mem::take(&mut *crate::lock(&self.state.moved));

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
