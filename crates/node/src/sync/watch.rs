//! Watching project folders for changes, with one watcher per node.
//!
//! On Linux that is one inotify instance, and each folder costs one watch
//! against the user's `fs.inotify.max_user_watches`, so only the folders
//! sync keeps are watched (never `target/` or `node_modules/`), and a
//! project's folders are counted against what is left before they are
//! watched. A project that does not fit, or whose watch fails with
//! `ENOSPC`, is rescanned every `RESCAN` instead, and the user is told once
//! how to raise the limit. On macOS one FSEvents stream per project folder
//! covers the whole tree, so there is nothing to count.

use super::rules::Rules;
use crate::lock;
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _,
    event::{AccessKind, AccessMode},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, RwLock},
    time::Duration,
};
use tokio::{sync::mpsc, task::spawn_blocking};

/// How often a project that cannot be watched is rescanned.
pub(super) const RESCAN: Duration = Duration::from_secs(45);
/// The share of the watch limit left to other programs, in percent, and
/// at least `MIN_RESERVE` watches.
const RESERVE_PERCENT: u64 = 10;
const MIN_RESERVE: u64 = 512;

/// What raises the watch limit.
const WATCHES_HINT: &str = "Beep rescans this folder every 45 s because the inotify watch limit \
    is reached. Raise it with `sudo sysctl fs.inotify.max_user_watches=524288`, and add \
    `fs.inotify.max_user_watches=524288` to a file in /etc/sysctl.d/ to keep it.";
const INSTANCES_HINT: &str = "Beep rescans this folder every 45 s because no inotify instance \
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

/// The watches one node may still add.
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

    /// Takes in the latest count of other programs' watches.
    fn others_now(&mut self, others: u64) {
        self.others = others;
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

/// Where events for a project go.
struct Route {
    root: PathBuf,
    rules: Arc<RwLock<Rules>>,
    wake: mpsc::Sender<()>,
}

/// One project's watches.
struct Watched {
    root: PathBuf,
    dirs: HashSet<PathBuf>,
    mode: Mode,
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

fn route(routes: &Mutex<HashMap<Key, Route>>, event: &Event) {
    if !is_change(&event.kind) {
        return;
    }
    let routes = lock(routes);
    for path in &event.paths {
        let Some(route) = routes
            .values()
            .filter(|route| path.starts_with(&route.root))
            .max_by_key(|route| route.root.as_os_str().len())
        else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(&route.root) else {
            continue;
        };
        let skipped = route
            .rules
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .skips_any(relative, path.is_dir());
        if !skipped {
            // Full means a scan is due already.
            let _ = route.wake.try_send(());
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
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) => route(&events, &event),
                Err(e) => log::debug!("watch error: {e}"),
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
            },
        );
        lock(&self.inner).projects.insert(
            key,
            Watched {
                root: root.to_owned(),
                dirs: HashSet::new(),
                mode: Mode::Watching,
            },
        );
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
        watched.mode = mode.clone();
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
            budget.others_now(recount());
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
            // It went away since the scan; the next scan sees that.
            Err(e) => log::debug!("could not watch {}: {e}", dir.display()),
        }
    }
    Mode::Watching
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_and_reading_files_is_not_a_change() {
        use notify::event::{CreateKind, ModifyKind};
        assert!(!is_change(&EventKind::Access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(!is_change(&EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
        assert!(is_change(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(is_change(&EventKind::Create(CreateKind::File)));
        assert!(is_change(&EventKind::Modify(ModifyKind::Any)));
    }

    #[tokio::test]
    async fn a_new_file_wakes_its_project() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let watcher = Watcher::with_budget(None, None);
        let rules = Arc::new(RwLock::new(Rules::load(&root).unwrap()));
        let (wake, mut woken) = mpsc::channel(1);
        watcher.add(1, &root, rules, wake);
        assert_eq!(
            watcher.set_dirs(1, vec![root.clone()]).await,
            Mode::Watching
        );
        std::fs::write(root.join("notes.txt"), "x").unwrap();
        // The watcher's own thread delivers it; nothing else to wait on.
        let woke = tokio::time::timeout(Duration::from_secs(10), woken.recv()).await;
        assert_eq!(woke.expect("woken in time"), Some(()));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_project_that_does_not_fit_is_rescanned_and_others_still_watch() {
        let dir = tempfile::TempDir::new().unwrap();
        let dirs: Vec<PathBuf> = (0..4)
            .map(|n| {
                let d = dir.path().join(format!("d{n}"));
                std::fs::create_dir(&d).unwrap();
                d
            })
            .collect();
        // Room for three folders beside the reserve.
        let watcher = Watcher::with_budget(Some(Budget::new(MIN_RESERVE + 3, 0)), None);
        let rules = Arc::new(RwLock::new(Rules::load(dir.path()).unwrap()));
        let (wake, _woken) = mpsc::channel(1);
        watcher.add(1, dir.path(), rules.clone(), wake.clone());
        watcher.add(2, dir.path(), rules, wake);
        assert_eq!(
            watcher.set_dirs(1, dirs[..2].to_vec()).await,
            Mode::Watching
        );
        let mode = watcher.set_dirs(2, dirs[2..].to_vec()).await;
        let Mode::Rescan { hint } = mode else {
            panic!("the second project does not fit");
        };
        assert!(hint.contains("fs.inotify.max_user_watches"));
        // The first keeps its watches, and the second fits once it left.
        assert_eq!(
            watcher.set_dirs(1, dirs[..2].to_vec()).await,
            Mode::Watching
        );
        watcher.remove(1).await;
        assert_eq!(
            watcher.set_dirs(2, dirs[2..].to_vec()).await,
            Mode::Watching
        );
    }

    /// Lets the counting test's `recount` finish, from an async task.
    #[cfg(target_os = "linux")]
    static RECOUNTED: Mutex<Option<std::sync::mpsc::Receiver<()>>> = Mutex::new(None);

    /// Counts other programs' watches, as long as an async task on the same
    /// worker can run meanwhile: true when it did.
    #[cfg(target_os = "linux")]
    fn recount_beside_a_task() -> u64 {
        let rx = lock(&RECOUNTED).take().expect("the test set it");
        u64::from(rx.recv_timeout(Duration::from_secs(5)).is_err())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn counting_watches_leaves_the_async_worker_free() {
        let dir = tempfile::TempDir::new().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        *lock(&RECOUNTED) = Some(rx);
        let watcher =
            Watcher::with_budget(Some(Budget::new(100_000, 0)), Some(recount_beside_a_task));
        let rules = Arc::new(RwLock::new(Rules::load(dir.path()).unwrap()));
        let (wake, _woken) = mpsc::channel(1);
        watcher.add(1, dir.path(), rules, wake);
        // Runs on the only worker only if counting does not hold it.
        let beside = tokio::spawn(async move { tx.send(()).unwrap() });
        let mode = watcher.set_dirs(1, vec![dir.path().to_owned()]).await;
        beside.await.unwrap();
        assert_eq!(mode, Mode::Watching);
        // The recount saw the task's message, so it counted 0 others.
        let inner = lock(&watcher.inner);
        assert_eq!(inner.budget.as_ref().unwrap().others, 0);
    }
}
