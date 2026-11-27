use super::*;

#[tokio::test]
async fn the_walk_publishes_after_it_ends_and_lets_a_rescan_run() {
    let home = tempfile::tempdir().expect("a temporary folder");
    let dir = tempfile::tempdir().expect("a temporary folder");
    std::fs::create_dir_all(home.path().join("work/app/.git")).expect("a checkout");
    let agents = Agents::new(Vec::new(), crate::occupancy::Occupancy::default());
    assert!(agents.disk_checkouts().is_err(), "nothing is known before a walk");

    agents
        .warm(Some(home.path().to_path_buf()), dir.path().to_path_buf())
        .await;

    assert!(agents.listings.rescanning.try_lock().is_ok());
    let found = agents.disk_checkouts().expect("the walk published its list");
    assert!(found.iter().any(|checkout| checkout.root.ends_with("work/app")));

    std::fs::create_dir_all(home.path().join("work/new/.git")).expect("a checkout");
    agents
        .rescan(home.path().to_path_buf(), dir.path().to_path_buf())
        .await
        .expect("the rescan runs");
    let found = agents.disk_checkouts().expect("the rescan published its list");
    assert!(found.iter().any(|checkout| checkout.root.ends_with("work/new")));
}

#[tokio::test]
async fn a_listing_for_one_limit_does_not_wait_for_another() {
    let agents = Agents::new(Vec::new(), crate::occupancy::Occupancy::default());
    agents.warm(None, std::path::PathBuf::new()).await;
    let slot = crate::lock(&agents.listings.listed).entry(5).or_default().clone();
    let _running = slot.lock().await;

    let listed = tokio::time::timeout(std::time::Duration::from_secs(20), agents.found_folders(10))
        .await
        .expect("a listing for another limit is not held up");

    assert!(listed.is_ok());
}

#[tokio::test]
async fn stored_sessions_lock_one_at_a_time_each_and_the_lock_is_forgotten_after() {
    let agents = Agents::new(Vec::new(), crate::occupancy::Occupancy::default());
    let (one, other) = (SessionId::from("one"), SessionId::from("other"));
    let first = agents.sessions.store_of(&one);
    let held = first.lock().await;

    let same = agents.sessions.store_of(&one);
    assert!(same.try_lock().is_err(), "the same session waits");
    assert!(
        agents.sessions.store_of(&other).try_lock().is_ok(),
        "another session does not"
    );

    drop((held, same));
    drop(first);
    let _other = agents.sessions.store_of(&other);
    assert_eq!(crate::lock(&agents.sessions.store).len(), 1);
}

#[tokio::test]
async fn the_warm_walk_waits_for_a_rescan_that_runs() {
    let agents = Arc::new(Agents::new(Vec::new(), crate::occupancy::Occupancy::default()));
    let rescan = agents.listings.rescanning.lock().await;
    let warm = tokio::spawn({
        let agents = agents.clone();
        async move { agents.warm(None, std::path::PathBuf::new()).await }
    });

    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(agents.disk_checkouts().is_err(), "warm walked beside the rescan");

    drop(rescan);
    warm.await.expect("warm ended");
    assert!(agents.disk_checkouts().is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn a_project_reached_through_a_symlink_lists_a_worktree_session_at_its_real_path() {
    let (codex, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let real = std::fs::canonicalize(data.path()).unwrap().join("proj");
    let tree = real.join(".worktrees/feature");
    std::fs::create_dir_all(&tree).unwrap();
    let link = data.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let sessions = codex.path().join("sessions/2026/11/01");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join("rollout-one.jsonl"),
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"one\",\"cwd\":{},\"source\":\"cli\"}}}}\n",
            serde_json::to_string(&tree.to_string_lossy()).unwrap()
        ),
    )
    .unwrap();
    let occupancy = crate::occupancy::Occupancy::over(
        crate::logs::claude::Store::default(),
        crate::logs::found::Stores::at(Some(codex.path()), None, None),
    );
    let agents = Agents::new(Vec::new(), occupancy);

    let listed = agents
        .sessions_in(vec![link.to_string_lossy().into_owned()], 10)
        .await
        .unwrap();

    assert_eq!(listed.len(), 1, "{listed:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_project_asked_by_its_real_path_lists_a_session_recorded_through_the_link() {
    let (codex, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let real = std::fs::canonicalize(data.path()).unwrap().join("proj");
    std::fs::create_dir_all(real.join("src")).unwrap();
    let link = data.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let sessions = codex.path().join("sessions/2026/11/01");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join("rollout-one.jsonl"),
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"one\",\"cwd\":{},\"source\":\"cli\"}}}}\n",
            serde_json::to_string(&link.join("src").to_string_lossy()).unwrap()
        ),
    )
    .unwrap();
    let occupancy = crate::occupancy::Occupancy::over(
        crate::logs::claude::Store::default(),
        crate::logs::found::Stores::at(Some(codex.path()), None, None),
    );
    let agents = Agents::new(Vec::new(), occupancy);

    let listed = agents
        .sessions_in(vec![real.to_string_lossy().into_owned()], 10)
        .await
        .unwrap();

    assert_eq!(listed.len(), 1, "{listed:?}");
}
