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

/// What a reader is told: the roots that changed, whether the change was structural, and the
/// paths that changed under them; `None` when they are not known, as after a missed event.
type Changed = Arc<dyn Fn(Vec<PathBuf>, bool, Option<Vec<PathBuf>>) + Send + Sync>;

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
    /// Every watch is to be made again: notify missed events.
    stale: AtomicBool,
    /// Roots whose own folder was removed or replaced, to watch again.
    lost: Mutex<Vec<PathBuf>>,
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
    /// Who asked, for the metrics.
    label: &'static str,
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

/// Folders under `.git` that change on every Git command and that no reader
/// follows. `.git` itself stays watched: Git replaces `.git/index` by rename.
const GIT_QUIET: &[&str] = &["objects", "logs", "lfs"];

static SHARED: Mutex<Option<Arc<Shared>>> = Mutex::new(None);
static NEXT_READER: AtomicU64 = AtomicU64::new(0);

impl Paths {
    pub(crate) fn new(
        reader: &'static str,
        changed: impl Fn(Vec<PathBuf>) + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::with_structure(reader, move |roots, _| changed(roots))
    }

    pub(crate) fn with_structure(
        reader: &'static str,
        changed: impl Fn(Vec<PathBuf>, bool) + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::with_paths(reader, move |roots, structural, _| changed(roots, structural))
    }

    /// Like `with_structure`, and says which paths changed under the roots when it knows.
    pub(crate) fn with_paths(
        reader: &'static str,
        changed: impl Fn(Vec<PathBuf>, bool, Option<Vec<PathBuf>>) + Send + Sync + 'static,
    ) -> Result<Self> {
        let shared = Shared::get()?;
        let id = NEXT_READER.fetch_add(1, Ordering::Relaxed);
        let changed: Changed = Arc::new(changed);
        crate::lock(&shared.state.readers).held.insert(
            id,
            Reader {
                label: reader,
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
        let self_changed = self.changed.clone();
        let (result, missed) = spawn_blocking(move || {
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
            drop(inner);

            if result.is_err() {
                shared.state.set(id, before.clone());
                shared.state.forget(&roots);
            }

            let mut missed = shared.state.missed(&result);

            if before != roots {
                missed.retain(|(reader, _, _)| *reader != id);
                let changed = before.into_iter().chain(roots).collect();
                missed.push((id, self_changed, changed));
            }
            Ok::<_, anyhow::Error>((result, missed))
        })
        .await
        .context("filesystem watcher stopped")??;

        for (_, changed, roots) in missed {
            changed(roots, true, None);
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
            lost: Mutex::default(),
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
            for (_, changed, roots) in self.state.missed(&result) {
                changed(roots, true, None);
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
                let paths = match &event {
                    Ok(event) if !event.need_rescan() => Some(
                        event
                            .paths
                            .iter()
                            .filter(|path| reader.roots.iter().any(|root| affects(root, path)))
                            .cloned()
                            .collect(),
                    ),
                    _ => None,
                };
                (reader.label, reader.changed.clone(), roots, paths)
            })
            .filter(|(_, _, roots, _): &(_, Changed, Vec<PathBuf>, _)| !roots.is_empty())
            .collect();
        let kind = kind_of(&event);

        if reached.is_empty() {
            metrics::counter!("beep_watch_reached_total", "reader" => "none", "kind" => kind)
                .increment(1);
        }

        for (label, _, _, _) in &reached {
            metrics::counter!("beep_watch_reached_total", "reader" => *label, "kind" => kind)
                .increment(1);
        }

        match &event {
            Ok(event) if !event.need_rescan() => {
                if renames(event) {
                    let roots = readers.watched();
                    let lost: Vec<_> = event
                        .paths
                        .iter()
                        .flat_map(|path| lost(&roots, path))
                        .cloned()
                        .collect();
                    crate::lock(&self.lost).extend(lost);
                }
            }
            _ => self.stale.store(true, Ordering::Release),
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

        for (_, changed, roots, paths) in reached {
            changed(roots, structural, paths);
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
        let mut roots = crate::lock(&self.readers).watched();
        roots.sort();
        roots.dedup();
        roots
    }

    /// The readers and roots a pass over the watches may have missed
    /// changes for, since adding an OS watch leaves a short gap: every root
    /// after a rebuild or a failure, else those under a watch just added.
    fn missed(&self, result: &Result<Installed>) -> Vec<(u64, Changed, Vec<PathBuf>)> {
        crate::lock(&self.readers)
            .held
            .iter()
            .filter_map(|(id, reader)| {
                let roots: Vec<_> = match result {
                    Err(_) | Ok(Installed { rebuilt: true, .. }) => reader.roots.clone(),
                    Ok(Installed { served, .. }) => reader
                        .roots
                        .iter()
                        .filter(|root| served.contains(root))
                        .cloned()
                        .collect(),
                };
                (!roots.is_empty()).then(|| (*id, reader.changed.clone(), roots))
            })
            .collect()
    }

    /// Stops `roots` lingering: they could not be watched, and each pass
    /// would only fail on them again.
    fn forget(&self, roots: &[PathBuf]) {
        let mut readers = crate::lock(&self.readers);

        for root in roots {
            readers.lingering.remove(root);
        }
    }
}

impl Readers {
    /// Every root a watch is held for: wanted by a reader, or lingering.
    fn watched(&self) -> Vec<PathBuf> {
        let mut roots = self.roots();
        roots.extend(self.lingering.keys().cloned());
        roots
    }

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

/// What one pass over the watches did.
#[derive(Default)]
struct Installed {
    /// Every watch was made again, after one was lost.
    rebuilt: bool,
    /// The roots whose watch was made in this pass.
    served: Vec<PathBuf>,
}

impl Inner {
    fn install(&mut self, roots: &[PathBuf]) -> Result<Installed> {
        let mut wanted = BTreeMap::new();
        let anchors: Vec<_> = roots
            .iter()
            .filter_map(|root| existing(root).ok().map(|anchor| (root, anchor)))
            .collect();

        for (_, (path, mode)) in anchors.iter().cloned() {
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
        // A root's folder replaced at the same path keeps its entry but not
        // its OS watch: it is made again, and only it.
        let lost: Vec<_> = std::mem::take(&mut *crate::lock(&self.state.lost))
            .iter()
            .filter_map(|root| existing(root).ok().map(|(path, _)| path))
            .collect();
        let again = |path: &PathBuf| stale || lost.contains(path);

        if self.installed == wanted && !stale && lost.is_empty() {
            return Ok(Installed::default());
        }
        // notify owns OS watch descriptors by path, without caller reference
        // counts: watches that go are removed before overlapping ones are
        // added. Only what changed or was lost is walked again.
        let gone: Vec<_> = self
            .installed
            .iter()
            .filter(|(path, mode)| wanted.get(*path) != Some(*mode) || again(path))
            .map(|(path, mode)| (path.clone(), *mode))
            .collect();
        let added: Vec<_> = wanted
            .iter()
            .filter(|(path, mode)| self.installed.get(*path) != Some(*mode) || again(path))
            .map(|(path, mode)| (path.clone(), *mode))
            .collect();
        let served = anchors
            .iter()
            .filter(|(_, (anchor, _))| {
                added.iter().any(|(path, mode)| {
                    path == anchor || (*mode == RecursiveMode::Recursive && covers(path, anchor))
                })
            })
            .map(|(root, _)| (*root).clone())
            .collect();

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
        Ok(Installed {
            rebuilt: stale,
            served,
        })
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
                    !entry.file_name().to_str().is_some_and(|name| {
                        UNWATCHED.contains(&name)
                            || (folder.file_name().is_some_and(|parent| parent == ".git")
                                && GIT_QUIET.contains(&name))
                    })
                })
                .map(|entry| entry.path()),
        );
    }

    Ok(())
}

/// Counts one filesystem notification by the watcher that took it.
pub(crate) fn counted(watcher: &'static str, event: &notify::Result<notify::Event>) {
    metrics::counter!("beep_watch_events_total", "watcher" => watcher, "kind" => kind_of(event))
        .increment(1);
}

fn kind_of(event: &notify::Result<notify::Event>) -> &'static str {
    match event {
        Ok(event) if event.need_rescan() => "rescan",
        Ok(event) => match event.kind {
            EventKind::Create(_) => "create",
            EventKind::Modify(_) => "modify",
            EventKind::Remove(_) => "remove",
            EventKind::Access(_) => "access",
            EventKind::Any | EventKind::Other => "other",
        },
        Err(_) => "error",
    }
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
    let mut after_git = false;

    relative.components().any(|component| {
        let name = component.as_os_str().to_str();
        let quiet = after_git && name.is_some_and(|name| GIT_QUIET.contains(&name));
        after_git = name == Some(".git");

        quiet || name.is_some_and(|name| UNWATCHED.contains(&name))
    })
}

/// The roots whose watch replacing `path` drops: it is, or holds, a root no
/// other root outside it covers. Git replaces a checkout's `.git/index` on
/// most commands, and the recursive watch of the checkout still covers it, so
/// that alone does not watch it again.
fn lost<'a>(roots: &'a [PathBuf], path: &'a Path) -> impl Iterator<Item = &'a PathBuf> {
    roots.iter().filter(move |root| {
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
