use super::*;

#[tokio::test]
async fn a_missing_directory_only_watches_its_parent_non_recursively() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new/store");
    let watcher = Paths::new(|_| {}).unwrap();
    watcher.replace(vec![path]).await.unwrap();
    let held = crate::lock(&watcher.inner);
    assert_eq!(held.installed.len(), 1);
    assert_eq!(
        held.installed
            .get(&crate::agents::reach::real(root.path()).unwrap()),
        Some(&RecursiveMode::NonRecursive)
    );
}

#[tokio::test]
async fn aliases_share_one_watch_and_removing_one_keeps_the_directory_watched() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(not(unix))]
    let aliases = vec![root.path().to_owned(), root.path().to_owned()];
    #[cfg(unix)]
    let (_alias_directory, aliases) = {
        let directory = tempfile::tempdir().unwrap();
        let alias = directory.path().join("alias");
        std::os::unix::fs::symlink(root.path(), &alias).unwrap();
        (directory, vec![root.path().to_owned(), alias])
    };
    let (changed, mut changes) = mpsc::channel(8);
    let watcher = Paths::new(move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    watcher.replace(aliases).await.unwrap();
    watcher.replace(vec![root.path().to_owned()]).await.unwrap();
    assert_eq!(crate::lock(&watcher.inner).installed.len(), 1);
    while changes.try_recv().is_ok() {}
    std::fs::write(root.path().join("example.txt"), "changed").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), changes.recv())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn renaming_and_recreating_a_root_rearms_the_replacement_directory() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("watched");
    std::fs::create_dir(&root).unwrap();
    let (changed, mut changes) = mpsc::channel(32);
    let watcher = Paths::new(move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    watcher.replace(vec![root.clone()]).await.unwrap();
    while changes.try_recv().is_ok() {}
    let inner = watcher.inner.clone();
    let (ready, wait_ready) = tokio::sync::oneshot::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let original = root.clone();
    let renamed = directory.path().join("old");
    let hold = tokio::task::spawn_blocking(move || {
        let _held = crate::lock(&inner);
        std::fs::rename(&original, renamed).unwrap();
        std::fs::create_dir(&original).unwrap();
        ready.send(()).unwrap();
        wait_release
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    });
    wait_ready.await.unwrap();
    timeout_change(&mut changes).await;
    let mut reconciled = watcher.reconciled.clone();
    reconciled.borrow_and_update();
    release.send(()).unwrap();
    hold.await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), reconciled.changed())
        .await
        .unwrap()
        .unwrap();
    while changes.try_recv().is_ok() {}
    std::fs::write(root.join("after.txt"), "changed").unwrap();
    timeout_change(&mut changes).await;
}

async fn timeout_change(changes: &mut mpsc::Receiver<Vec<PathBuf>>) {
    tokio::time::timeout(std::time::Duration::from_secs(5), changes.recv())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn a_directory_created_after_subscribing_becomes_recursive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new/store");
    let (changed, mut changes) = mpsc::channel(32);
    let watcher = Paths::new(move |paths| {
        let _ = changed.try_send(paths);
    })
    .unwrap();
    watcher.replace(vec![path.clone()]).await.unwrap();
    std::fs::create_dir_all(&path).unwrap();
    let mut reconciled = watcher.reconciled.clone();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            reconciled.borrow_and_update();
            if crate::lock(&watcher.inner)
                .installed
                .get(&crate::agents::reach::real(&path).unwrap())
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
    tokio::time::timeout(std::time::Duration::from_secs(5), changes.recv())
        .await
        .unwrap()
        .unwrap();
}
