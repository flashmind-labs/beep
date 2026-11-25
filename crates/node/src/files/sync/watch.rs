//! Watches project folders for changes with one `Watcher` per node. A
//! folder it cannot watch is rescanned every `RESCAN`. Healthy watchers
//! retain changed paths; overflow and errors request reconciliation.

use super::{gitdir, rules::Rules};
use crate::{lock, read_lock};
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _,
    event::{AccessKind, AccessMode},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};
use crate::events::{ChangeKind, Touched};
use beep_proto::node::events::Topic;
use tokio::{
    sync::{Notify, mpsc},
    task::spawn_blocking,
};
use tokio_util::task::AbortOnDropHandle;

/// How often a project the watcher cannot follow is rescanned.
pub(super) const RESCAN: Duration = Duration::from_secs(5 * 60);
/// The least time between two change signals for one project's files.
/// Sync itself waits for the folder to go quiet; this only paces listings.
const ANNOUNCE_GAP: Duration = Duration::from_millis(250);
/// The share of the watch limit left to other programs, in percent, and
/// at least `MIN_RESERVE` watches.
const RESERVE_PERCENT: u64 = 10;
const MIN_RESERVE: u64 = 512;

/// What raises the watch limit.
const WATCHES_HINT: &str = "Beep rescans this folder every 5 minutes because the inotify watch limit \
    is reached. Raise it with `sudo sysctl fs.inotify.max_user_watches=524288`, and add \
    `fs.inotify.max_user_watches=524288` to a file in /etc/sysctl.d/ to keep it.";
const INSTANCES_HINT: &str = "Beep rescans this folder every 5 minutes because no inotify instance \
    is left. Raise the limit with `sudo sysctl fs.inotify.max_user_instances=512`, and add \
    `fs.inotify.max_user_instances=512` to a file in /etc/sysctl.d/ to keep it.";

/// Which project a watch belongs to.
pub(super) type Key = u64;

/// How a project learns of changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Mode {
    Watching,
    /// Rescanned every `RESCAN`, with what raises the limit.
    Rescan {
        hint: &'static str,
    },
}

/// The watches one node may still add. On Linux each watched folder costs
/// one inotify watch against `fs.inotify.max_user_watches`, so only folders
/// sync keeps are watched and a project is counted against what is left
/// first. One that does not fit, or hits `ENOSPC`, is rescanned instead and
/// the user is told once how to raise the limit. macOS needs no budget: one
/// FSEvents stream covers a project's whole tree.
#[derive(Debug)]
pub(super) struct Budget {
    /// The user's limit.
    limit: u64,
    /// Watches other programs of the user hold.
    others: u64,
    /// Watches each project holds.
    used: BTreeMap<Key, u64>,
}

impl Budget {
    pub(super) fn new(limit: u64, others: u64) -> Self {
        Self {
            limit,
            others,
            used: BTreeMap::new(),
        }
    }

    fn reserve(&self) -> u64 {
        (self.limit * RESERVE_PERCENT / 100).max(MIN_RESERVE)
    }

    /// Watches still free for projects.
    pub(super) fn left(&self) -> u64 {
        let used: u64 = self.used.values().sum();
        self.limit
            .saturating_sub(self.reserve())
            .saturating_sub(self.others)
            .saturating_sub(used)
    }

    /// Lets `key` hold `dirs` watches if they fit beside the others'.
    pub(super) fn admit(&mut self, key: Key, dirs: u64) -> bool {
        let held = self.used.get(&key).copied().unwrap_or(0);

        if dirs > held && dirs - held > self.left() {
            return false;
        }

        self.used.insert(key, dirs);
        true
    }

    pub(super) fn release(&mut self, key: Key) {
        self.used.remove(&key);
    }
}

/// The limit and the watches other processes of this user hold, read from
/// `/proc`; `None` where there is no inotify.
fn read_budget() -> Option<Budget> {
    let limit = std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches").ok()?;
    let limit = limit.trim().parse().ok()?;
    Some(Budget::new(limit, others_watches()))
}

/// The inotify watches other processes of this user hold. Processes this
/// node cannot read (other users') are not counted, and do not count
/// against this user's limit either.
fn others_watches() -> u64 {
    let me = std::process::id().to_string();
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return 0;
    };
    let mut count = 0;

    for process in processes.flatten() {
        let name = process.file_name();
        let Some(pid) = name
            .to_str()
            .filter(|n| n.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };

        if pid == me {
            continue;
        }

        let Ok(fds) = std::fs::read_dir(process.path().join("fd")) else {
            continue;
        };

        for fd in fds.flatten() {
            let is_inotify = std::fs::read_link(fd.path())
                .is_ok_and(|target| target.as_os_str() == "anon_inode:inotify");
            if !is_inotify {
                continue;
            }

            let info = process.path().join("fdinfo").join(fd.file_name());

            if let Ok(info) = std::fs::read_to_string(info) {
                count += info
                    .lines()
                    .filter(|l| l.starts_with("inotify wd:"))
                    .count() as u64;
            }
        }
    }

    count
}

/// Signals `files/<root>` for changed projects: the first change at once,
/// the rest of a burst once after `ANNOUNCE_GAP`, each root once per signal.
#[derive(Default)]
struct Announcer {
    pending: Mutex<HashMap<PathBuf, (ChangeKind, Touched)>>,
    wake: Notify,
    /// Without a runtime to pace them, signals go out at once.
    paced: std::sync::atomic::AtomicBool,
}

impl Announcer {
    /// The announcer, pacing its signals on the current runtime when there is one.
    fn start() -> (Arc<Self>, Option<AbortOnDropHandle<()>>) {
        let announcer = Arc::new(Self::default());
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return (announcer, None);
        };
        announcer
            .paced
            .store(true, std::sync::atomic::Ordering::Release);
        let paced = announcer.clone();
        let task = runtime.spawn(async move {
            loop {
                paced.wake.notified().await;
                let pending = std::mem::take(&mut *lock(&paced.pending));

                for (root, (kind, touched)) in pending {
                    signal(&root, kind, touched);
                }

                tokio::time::sleep(ANNOUNCE_GAP).await;
            }
        });
        (announcer, Some(AbortOnDropHandle::new(task)))
    }

    fn changed(&self, root: &Path, kind: ChangeKind, touched: Touched) {
        if !self.paced.load(std::sync::atomic::Ordering::Acquire) {
            signal(root, kind, touched);
            return;
        }

        let mut pending = lock(&self.pending);
        let held = pending
            .entry(root.to_owned())
            .or_insert((kind, Touched::Nothing));
        held.0 = held.0.max(kind);
        held.1.add(touched);
        drop(pending);
        self.wake.notify_one();
    }
}

fn signal(root: &Path, kind: ChangeKind, touched: Touched) {
    let topic = Topic::keyed("files", root.to_string_lossy());
    crate::events::system().changed_touching(topic, kind, touched);
}

/// Where events for a project go.
struct Route {
    root: PathBuf,
    rules: Arc<RwLock<Rules>>,
    wake: mpsc::Sender<()>,
    changes: Changes,
}

/// Paths are retained even when the wake channel is already full.
#[derive(Debug, Default)]
pub(super) struct Changes {
    pub(super) paths: BTreeSet<PathBuf>,
    pub(super) full: bool,
}

impl Changes {
    fn insert(&mut self, path: PathBuf) {
        if self.full {
            return;
        }

        if path.as_os_str().is_empty() || self.paths.len() >= 4096 {
            self.invalidate();
            return;
        }

        if !self.paths.iter().any(|parent| path.starts_with(parent)) {
            self.paths.retain(|child| !child.starts_with(&path));
            self.paths.insert(path);
        }
    }

    fn invalidate(&mut self) {
        self.full = true;
        self.paths.clear();
    }
}

/// One project's watches.
struct Watched {
    root: PathBuf,
    dirs: HashSet<PathBuf>,
}

struct Inner {
    /// `None` when no watcher could be made; every project rescans then.
    watcher: Option<RecommendedWatcher>,
    budget: Option<Budget>,
    /// Counts other programs' watches again; none in tests.
    recount: Option<fn() -> u64>,
    projects: HashMap<Key, Watched>,
    /// The limit was logged.
    told: bool,
}

/// The node's one watcher.
#[derive(Clone)]
pub(super) struct Watcher {
    inner: Arc<Mutex<Inner>>,
    // Separate from `inner`: the watcher's thread takes it for every
    // event, and must never wait on a lock held around `watch`.
    routes: Arc<Mutex<HashMap<Key, Route>>>,
    /// Paces the change signals; dropped with the last clone.
    _announcing: Option<Arc<AbortOnDropHandle<()>>>,
}

/// Whether an event can mean a change: opening and reading files, as a
/// scan does, cannot.
fn is_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

fn route(routes: &Mutex<HashMap<Key, Route>>, announcer: &Announcer, event: &Event) {
    // Most events are opens and reads: leave them before the lock.
    if !event.need_rescan() && !is_change(&event.kind) {
        return;
    }

    let mut routes = lock(routes);
    // The system dropped events (a full inotify queue, an FSEvents
    // overflow): any folder may have changed, so every project scans.
    if event.need_rescan() {
        for route in routes.values_mut() {
            route.changes.invalidate();
            let _ = route.wake.try_send(());
            announcer.changed(&route.root, ChangeKind::Any, Touched::All);
        }

        return;
    }

    for path in &event.paths {
        let Some(route) = routes
            .values_mut()
            .filter(|route| path.starts_with(&route.root))
            .max_by_key(|route| route.root.as_os_str().len())
        else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(&route.root) else {
            continue;
        };
        // Git writes objects before the ref that names them, and packs them
        // up without a word to anyone: the ref's move is the signal.
        let git_says = gitdir::wakes(relative);
        let skipped = read_lock(&route.rules).skips_any(relative, path.is_dir());

        let rules_changed = super::rules::changes_rules(relative);
        if rules_changed {
            route.changes.invalidate();
        }

        if git_says || rules_changed || (!skipped && !gitdir::is_quiet(&relative.to_string_lossy()))
        {
            route.changes.insert(relative.to_owned());
            // Full means a scan is due already.
            let _ = route.wake.try_send(());
            let kind = if gitdir::is_metadata(relative) {
                ChangeKind::Any
            } else {
                ChangeKind::WorkTree
            };
            let touched = match relative.to_str() {
                Some(path) if !path.is_empty() => {
                    Touched::of([path.replace(std::path::MAIN_SEPARATOR, "/")])
                }
                _ => Touched::All,
            };
            announcer.changed(&route.root, kind, touched);
        }
    }
}

impl Watcher {
    /// The node's watcher, with the user's watch budget where it has one.
    pub(super) fn new() -> Self {
        Self::with_budget(read_budget(), Some(others_watches))
    }

    pub(super) fn with_budget(budget: Option<Budget>, recount: Option<fn() -> u64>) -> Self {
        let routes: Arc<Mutex<HashMap<Key, Route>>> = Arc::default();
        let events = routes.clone();
        let (announcer, announcing) = Announcer::start();
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| {
                crate::events::paths::counted("sync", &event);

                match event {
                Ok(event) => route(&events, &announcer, &event),
                Err(e) => {
                    log::debug!("watch error: {e}");
                    route(
                        &events,
                        &announcer,
                        &Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan),
                    );
                }
                }
            });
        let watcher = match watcher {
            Ok(watcher) => Some(watcher),
            Err(e) => {
                log::warn!("not watching project folders, they are rescanned instead: {e}");
                None
            }
        };
        Self {
            inner: Arc::new(Mutex::new(Inner {
                watcher,
                budget,
                recount,
                projects: HashMap::new(),
                told: false,
            })),
            routes,
            _announcing: announcing.map(Arc::new),
        }
    }

    /// Starts sending `wake` a message when something changes under
    /// `root` that `rules` keep. The folders come with `set_dirs`.
    pub(super) fn add(
        &self,
        key: Key,
        root: &Path,
        rules: Arc<RwLock<Rules>>,
        wake: mpsc::Sender<()>,
    ) {
        lock(&self.routes).insert(
            key,
            Route {
                root: root.to_owned(),
                rules,
                wake,
                changes: Changes::default(),
            },
        );
        lock(&self.inner).projects.insert(
            key,
            Watched {
                root: root.to_owned(),
                dirs: HashSet::new(),
            },
        );
    }

    /// Takes the accumulated paths. Events arriving during processing form the next batch.
    pub(super) fn take_changes(&self, key: Key) -> Changes {
        lock(&self.routes)
            .get_mut(&key)
            .map(|route| std::mem::take(&mut route.changes))
            .unwrap_or_default()
    }

    pub(super) fn recheck(&self, key: Key, paths: impl IntoIterator<Item = PathBuf>) {
        if let Some(route) = lock(&self.routes).get_mut(&key) {
            for path in paths {
                route.changes.insert(path);
            }
            let _ = route.wake.try_send(());
        }
    }

    /// Stops watching the project, off the async workers.
    pub(super) async fn remove(&self, key: Key) {
        let this = self.clone();

        if let Err(e) = spawn_blocking(move || this.remove_now(key)).await {
            log::warn!("unwatching a project failed: {e}");
        }
    }

    fn remove_now(&self, key: Key) {
        lock(&self.routes).remove(&key);
        let mut inner = lock(&self.inner);

        if let Some(watched) = inner.projects.remove(&key) {
            unwatch_all(&mut inner, &watched);
        }

        if let Some(budget) = &mut inner.budget {
            budget.release(key);
        }
    }

    /// Watches exactly `dirs`, the folders a scan of the project kept, and
    /// says how the project learns of changes now. A project that did not
    /// fit tries again with each call. Counting other programs' watches
    /// walks `/proc` and each watch is a system call, so it runs off the
    /// async workers.
    pub(super) async fn set_dirs(&self, key: Key, dirs: Vec<PathBuf>) -> Mode {
        let this = self.clone();

        match spawn_blocking(move || this.set_dirs_now(key, &dirs)).await {
            Ok(mode) => mode,
            Err(e) => {
                log::warn!("watching a project failed: {e}");
                Mode::Rescan { hint: WATCHES_HINT }
            }
        }
    }

    fn set_dirs_now(&self, key: Key, dirs: &[PathBuf]) -> Mode {
        let mut guard = lock(&self.inner);
        let inner = &mut *guard;
        let Some(mut watched) = inner.projects.remove(&key) else {
            return Mode::Watching;
        };
        let mode = watch_dirs(inner, key, &mut watched, dirs);

        if let Mode::Rescan { hint } = &mode
            && !inner.told
        {
            inner.told = true;
            log::warn!("{} is not watched: {hint}", watched.root.display());
        }

        inner.projects.insert(key, watched);
        mode
    }
}

fn unwatch_all(inner: &mut Inner, watched: &Watched) {
    let Some(watcher) = &mut inner.watcher else {
        return;
    };
    if cfg!(target_os = "linux") {
        for dir in &watched.dirs {
            // A folder that went away took its watch with it.
            let _ = watcher.unwatch(dir);
        }
    } else if !watched.dirs.is_empty() {
        let _ = watcher.unwatch(&watched.root);
    }
}

fn fall_back(inner: &mut Inner, key: Key, watched: &mut Watched, hint: &'static str) -> Mode {
    unwatch_all(inner, watched);
    watched.dirs.clear();

    if let Some(budget) = &mut inner.budget {
        budget.release(key);
    }
    Mode::Rescan { hint }
}

fn watch_dirs(inner: &mut Inner, key: Key, watched: &mut Watched, dirs: &[PathBuf]) -> Mode {
    if inner.watcher.is_none() {
        return Mode::Rescan {
            hint: INSTANCES_HINT,
        };
    }

    if !cfg!(target_os = "linux") {
        // One stream covers the tree; events are filtered by the rules.
        if watched.dirs.is_empty()
            && let Some(watcher) = &mut inner.watcher
        {
            match watcher.watch(&watched.root, RecursiveMode::Recursive) {
                Ok(()) => {
                    watched.dirs.insert(watched.root.clone());
                }
                Err(e) => {
                    log::warn!("could not watch {}: {e}", watched.root.display());
                    return Mode::Rescan { hint: WATCHES_HINT };
                }
            }
        }

        return Mode::Watching;
    }

    let wanted: HashSet<&PathBuf> = dirs.iter().collect();
    let recount = inner.recount;

    if let Some(budget) = &mut inner.budget {
        // Other programs come and go; count them again on each change.
        if wanted.len() != watched.dirs.len()
            && let Some(recount) = recount
        {
            budget.others = recount();
        }

        if !budget.admit(key, wanted.len() as u64) {
            return fall_back(inner, key, watched, WATCHES_HINT);
        }
    }

    let Some(watcher) = &mut inner.watcher else {
        return Mode::Watching;
    };
    let gone: Vec<PathBuf> = watched
        .dirs
        .iter()
        .filter(|dir| !wanted.contains(dir))
        .cloned()
        .collect();
    for dir in gone {
        let _ = watcher.unwatch(&dir);
        watched.dirs.remove(&dir);
    }

    for dir in wanted {
        if watched.dirs.contains(dir) {
            continue;
        }

        match watcher.watch(dir, RecursiveMode::NonRecursive) {
            Ok(()) => {
                watched.dirs.insert(dir.clone());
            }
            Err(e) if matches!(e.kind, notify::ErrorKind::MaxFilesWatch) => {
                return fall_back(inner, key, watched, WATCHES_HINT);
            }
            Err(e) => {
                log::debug!("could not watch {}: {e}", dir.display());
                return fall_back(inner, key, watched, WATCHES_HINT);
            }
        }
    }

    Mode::Watching
}

#[cfg(test)]
#[path = "../../tests/files_sync_watch.rs"]
mod tests;
