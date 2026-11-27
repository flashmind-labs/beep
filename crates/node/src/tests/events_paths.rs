use super::*;
use beep_proto::watched;
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::test]
async fn a_missing_directory_only_watches_its_parent_non_recursively() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new/store");
    let watcher = Paths::new("test", |_| {}).unwrap();
    watcher.replace(vec![path.clone()]).await.unwrap();
    let held = crate::lock(&watcher.shared.inner);
    assert!(!held.installed.contains_key(&path));
    assert_eq!(
        held.installed
            .get(&crate::fs::paths::real(root.path()).unwrap()),
        Some(&RecursiveMode::NonRecursive)
    );
}

#[tokio::test]
async fn renaming_and_recreating_a_root_rearms_the_replacement_directory() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("watched");
    std::fs::create_dir(&root).unwrap();
    let (changed, mut changes) = mpsc::channel(32);
    let watcher = Paths::new("test", move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    watcher.replace(vec![root.clone()]).await.unwrap();
    while changes.try_recv().is_ok() {}
    let shared = watcher.shared.clone();
    let (ready, wait_ready) = tokio::sync::oneshot::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let original = root.clone();
    let renamed = directory.path().join("old");
    let hold = tokio::task::spawn_blocking(move || {
        let _held = crate::lock(&shared.inner);
        std::fs::rename(&original, renamed).unwrap();
        std::fs::create_dir(&original).unwrap();
        ready.send(()).unwrap();
        wait_release.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    wait_ready.await.unwrap();
    timeout_change(&mut changes).await;
    let mut reconciled = watcher.reconciled.clone();
    watched::read_and_update(&mut reconciled, |_| ());
    release.send(()).unwrap();
    hold.await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), reconciled.changed())
        .await
        .unwrap()
        .unwrap();
    while changes.try_recv().is_ok() {}
    std::fs::write(root.join("after.txt"), "changed").unwrap();
    timeout_change(&mut changes).await;
}

async fn timeout_change(changes: &mut mpsc::Receiver<Vec<PathBuf>>) -> Vec<PathBuf> {
    tokio::time::timeout(Duration::from_secs(5), changes.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn a_directory_created_after_subscribing_becomes_recursive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new/store");
    let (changed, mut changes) = mpsc::channel(32);
    let watcher = Paths::new("test", move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    watcher.replace(vec![path.clone()]).await.unwrap();
    std::fs::create_dir_all(&path).unwrap();
    let mut reconciled = watcher.reconciled.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            watched::read_and_update(&mut reconciled, |_| ());

            if crate::lock(&watcher.shared.inner)
                .installed
                .get(&crate::fs::paths::real(&path).unwrap())
                == Some(&RecursiveMode::Recursive)
            {
                break;
            }

            reconciled.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    std::fs::write(path.join("example.txt"), "changed").unwrap();
    tokio::time::timeout(Duration::from_secs(5), changes.recv())
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn git_internals_do_not_touch_a_checkout_root_but_its_watched_refs_do() {
    let root = Path::new("/srv/project");
    assert!(affects(root, &root.join("src/main.rs")));
    assert!(!affects(root, &root.join(".git/objects/ab/cdef")));
    assert!(!affects(root, &root.join(".git/index")));
    assert!(!affects(root, &root.join("target/debug/app")));

    let refs = root.join(".git/refs");
    assert!(affects(&refs, &refs.join("heads/main")));
    let head = root.join(".git/HEAD");
    assert!(affects(&head, &head));
}

#[test]
fn replacing_a_file_in_a_watched_checkout_keeps_its_watch() {
    let checkout = PathBuf::from("/srv/project");
    let index = checkout.join(".git/index");
    let linked = PathBuf::from("/srv/elsewhere/.git/worktrees/x/index");
    let roots = vec![checkout.clone(), index.clone(), linked.clone()];

    assert_eq!(lost(&roots, &index).count(), 0);
    assert_eq!(lost(&roots, &linked).collect::<Vec<_>>(), [&linked]);
    assert_eq!(lost(&roots, &checkout).count(), 2);
    assert_eq!(lost(&roots, Path::new("/srv")).count(), 3);
}

#[test]
fn a_checkout_watch_leaves_out_linked_worktrees_and_build_output() {
    let root = Path::new("/srv/project");
    assert!(reports(root, &root.join("src")));
    assert!(!reports(root, &root.join(".worktrees/fix")));
    assert!(!reports(root, &root.join("app/node_modules/x")));
    assert!(!reports(root, root));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_walk_skips_build_output_and_keeps_a_worktree_inside_as_its_own_root() {
    let root = tempfile::tempdir().unwrap();
    let root = crate::fs::paths::real(root.path()).unwrap();
    for folder in ["a/b", "target/c", "app/node_modules/d", ".git/refs"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    let worktree = root.join(".worktrees/fix");
    std::fs::create_dir_all(worktree.join("src")).unwrap();
    let (changed, mut changes) = mpsc::channel(32);
    let watcher = Paths::new("test", move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    watcher
        .replace(vec![root.clone(), worktree.clone()])
        .await
        .unwrap();

    {
        let held = crate::lock(&watcher.shared.inner);
        let folders: Vec<_> = held.folders[&root]
            .iter()
            .map(|folder| folder.strip_prefix(&root).unwrap().to_owned())
            .collect();
        let expected: Vec<PathBuf> = ["", ".git", ".git/refs", "a", "a/b", "app"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(folders, expected);
        assert_eq!(held.folders[&worktree].len(), 2);
    }

    while changes.try_recv().is_ok() {}
    std::fs::write(worktree.join("src/main.rs"), "changed").unwrap();
    assert_eq!(timeout_change(&mut changes).await, [worktree]);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_folder_made_after_the_walk_is_watched_and_one_removed_is_let_go() {
    let root = tempfile::tempdir().unwrap();
    let root = crate::fs::paths::real(root.path()).unwrap();
    let (changed, mut changes) = mpsc::channel(32);
    let watcher = Paths::new("test", move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    watcher.replace(vec![root.clone()]).await.unwrap();
    let made = root.join("made");
    std::fs::create_dir(&made).unwrap();
    until_reconciled(&watcher, |held| held.folders[&root].contains(&made)).await;

    while changes.try_recv().is_ok() {}
    std::fs::write(made.join("example.txt"), "changed").unwrap();
    assert_eq!(
        timeout_change(&mut changes).await,
        std::slice::from_ref(&root)
    );

    std::fs::remove_dir_all(&made).unwrap();
    until_reconciled(&watcher, |held| !held.folders[&root].contains(&made)).await;
}

#[cfg(target_os = "linux")]
async fn until_reconciled(watcher: &Paths, done: impl Fn(&Inner) -> bool) {
    let mut reconciled = watcher.reconciled.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            watched::read_and_update(&mut reconciled, |_| ());

            if done(&crate::lock(&watcher.shared.inner)) {
                break;
            }

            reconciled.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_root_that_stops_lingering_leaves_other_readers_alone() {
    let gone = tempfile::tempdir().unwrap();
    let gone = crate::fs::paths::real(gone.path()).unwrap();
    let kept = tempfile::tempdir().unwrap();
    let first = Paths::new("test", |_| {}).unwrap();
    first.replace(vec![gone.clone()]).await.unwrap();
    let (changed, mut changes) = mpsc::channel(32);
    let second = Paths::new("test", move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    second.replace(vec![kept.path().to_owned()]).await.unwrap();
    let shared = second.shared.clone();
    drop(first);

    while changes.try_recv().is_ok() {}
    let mut reconciled = second.reconciled.clone();
    watched::read_and_update(&mut reconciled, |_| ());
    crate::lock(&shared.state.readers)
        .lingering
        .insert(gone.clone(), Instant::now());
    let _ = shared.state.refresh.try_send(());
    tokio::time::timeout(Duration::from_secs(5), async {
        while crate::lock(&shared.inner).installed.contains_key(&gone) {
            reconciled.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(changes.try_recv().is_err());
}

#[test]
fn git_folders_that_change_on_every_command_are_not_watched() {
    let root = Path::new("/srv/project");

    assert!(!reports(root, &root.join(".git/objects/ab/cdef")));
    assert!(!reports(root, &root.join(".git/logs/HEAD")));
    assert!(reports(root, &root.join(".git/index")));
    assert!(reports(root, &root.join("src/logs/today")));
    assert!(reports(root, &root.join("objects/model")));
}
