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
