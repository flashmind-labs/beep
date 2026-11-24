use super::*;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn the_walk_publishes_after_it_ends_and_lets_a_rescan_run() {
    let home = tempfile::tempdir().expect("a temporary folder");
    let dir = tempfile::tempdir().expect("a temporary folder");
    std::fs::create_dir_all(home.path().join("work/app/.git")).expect("a checkout");
    let agents = Agents::new(Vec::new());
    assert!(agents.disk_checkouts().is_err(), "nothing is known before a walk");

    agents
        .warm(Some(home.path().to_path_buf()), dir.path().to_path_buf())
        .await;

    assert!(!agents.rescanning.load(Ordering::Relaxed));
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
    let agents = Agents::new(Vec::new());
    agents.warm(None, std::path::PathBuf::new()).await;
    let slot = crate::lock(&agents.listed).entry(5).or_default().clone();
    let _running = slot.lock().await;

    let listed = tokio::time::timeout(std::time::Duration::from_secs(20), agents.found_folders(10))
        .await
        .expect("a listing for another limit is not held up");

    assert!(listed.is_ok());
}

#[tokio::test]
async fn a_reset_makes_the_next_read_of_the_logs_start_over() {
    let agents = Agents::new(Vec::new());
    let first = agents.sighted(|_| true).await.expect("a first read");
    let kept = agents.sighted(|_| true).await.expect("the kept read");
    assert!(Arc::ptr_eq(&first.seen, &kept.seen));

    agents.reset_sighted();

    let again = agents.sighted(|_| true).await.expect("a read after the reset");
    assert!(!Arc::ptr_eq(&first.seen, &again.seen));
}

#[tokio::test]
async fn stored_sessions_lock_one_at_a_time_each_and_the_lock_is_forgotten_after() {
    let agents = Agents::new(Vec::new());
    let (one, other) = (SessionId::from("one"), SessionId::from("other"));
    let first = agents.store_of(&one);
    let held = first.lock().await;

    let same = agents.store_of(&one);
    assert!(same.try_lock().is_err(), "the same session waits");
    assert!(
        agents.store_of(&other).try_lock().is_ok(),
        "another session does not"
    );

    drop((held, same));
    drop(first);
    let _other = agents.store_of(&other);
    assert_eq!(crate::lock(&agents.session_store).len(), 1);
}
