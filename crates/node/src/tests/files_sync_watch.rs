use super::*;
use crate::files::paths;

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

#[test]
fn dropped_events_wake_every_project() {
    use notify::event::Flag;
    let dir = tempfile::TempDir::new().unwrap();
    let routes: Mutex<HashMap<Key, Route>> = Mutex::default();
    let mut woken = Vec::new();

    for (key, root) in [(1, "/a"), (2, "/b")] {
        let (wake, rx) = mpsc::channel(1);
        let rules = Arc::new(RwLock::new(Rules::load(dir.path(), "").unwrap()));
        lock(&routes).insert(
            key,
            Route {
                root: PathBuf::from(root),
                rules,
                wake,
                changes: Changes::default(),
            },
        );
        woken.push(rx);
    }

    let event = Event::new(EventKind::Other).set_flag(Flag::Rescan);
    route(&routes, &Announcer::default(), &event);

    for rx in &mut woken {
        assert_eq!(rx.try_recv(), Ok(()));
    }
    assert!(lock(&routes).values().all(|route| route.changes.full));
}

#[test]
fn event_paths_survive_a_full_wake_channel_and_directory_changes_coalesce() {
    let dir = tempfile::TempDir::new().unwrap();
    let rules = Arc::new(RwLock::new(Rules::load(dir.path(), "").unwrap()));
    let (wake, mut woken) = mpsc::channel(1);
    let routes = Mutex::new(HashMap::from([(
        1,
        Route {
            root: dir.path().to_owned(),
            rules,
            wake,
            changes: Changes::default(),
        },
    )]));
    for path in ["one.txt", "sub/two.txt", "sub"] {
        route(
            &routes,
            &Announcer::default(),
            &Event::new(EventKind::Any).add_path(dir.path().join(path)),
        );
    }
    assert_eq!(woken.try_recv(), Ok(()));
    assert_eq!(
        lock(&routes)[&1].changes.paths,
        BTreeSet::from([PathBuf::from("one.txt"), PathBuf::from("sub")])
    );
    assert!(!lock(&routes)[&1].changes.full);
}

#[test]
fn changing_ignore_rules_and_overflow_require_reconciliation() {
    let mut changes = Changes::default();
    changes.insert(PathBuf::from("file.txt"));
    changes.insert(PathBuf::new());
    assert!(changes.full);
    assert!(changes.paths.is_empty());
    assert!(super::super::rules::changes_rules(Path::new(".stignore")));
    assert!(super::super::rules::changes_rules(Path::new(
        "sub/package.json"
    )));
    assert!(!super::super::rules::changes_rules(Path::new(
        "src/file.rs"
    )));
}

#[tokio::test]
async fn a_new_file_wakes_its_project() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = paths::canonicalize(dir.path()).unwrap();
    let watcher = Watcher::with_budget(None, None);
    let rules = Arc::new(RwLock::new(Rules::load(&root, "").unwrap()));
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
    let rules = Arc::new(RwLock::new(Rules::load(dir.path(), "").unwrap()));
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
    let watcher = Watcher::with_budget(Some(Budget::new(100_000, 0)), Some(recount_beside_a_task));
    let rules = Arc::new(RwLock::new(Rules::load(dir.path(), "").unwrap()));
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

#[tokio::test(start_paused = true)]
async fn a_burst_of_changes_signals_a_root_at_once_and_once_after_the_gap() {
    let root = PathBuf::from(format!("/srv/burst-{}", std::process::id()));
    let topic = Topic::keyed("files", root.to_string_lossy());
    let mut changes = crate::events::system().subscribe();
    let (announcer, _task) = Announcer::start();
    let mut signals = Vec::new();
    let collect = |changes: &mut tokio::sync::broadcast::Receiver<crate::events::Change>,
                   signals: &mut Vec<ChangeKind>| {
        while let Ok(change) = changes.try_recv() {
            if change.topic == topic {
                signals.push(change.kind);
            }
        }
    };

    announcer.changed(&root, ChangeKind::WorkTree, Touched::All);
    // Let the announcer take the first change before the burst follows it.
    tokio::time::sleep(ANNOUNCE_GAP / 10).await;

    for _ in 0..100 {
        announcer.changed(&root, ChangeKind::WorkTree, Touched::All);
    }
    announcer.changed(&root, ChangeKind::Any, Touched::All);
    collect(&mut changes, &mut signals);
    assert_eq!(signals, [ChangeKind::WorkTree]);

    tokio::time::sleep(ANNOUNCE_GAP).await;
    collect(&mut changes, &mut signals);
    // The trailing signal carries the strongest kind of the burst.
    assert_eq!(signals, [ChangeKind::WorkTree, ChangeKind::Any]);

    tokio::time::sleep(ANNOUNCE_GAP * 4).await;
    collect(&mut changes, &mut signals);
    assert_eq!(signals.len(), 2);
}
