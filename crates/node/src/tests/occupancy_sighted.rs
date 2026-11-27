use super::*;
use crate::logs::{claude::Store, found::Stores};

fn rollout(root: &std::path::Path, id: &str) {
    let sessions = root.join("sessions/2026/11/01");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("rollout-{id}.jsonl")),
        format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"/srv/project\",\"source\":\"cli\"}}}}\n"),
    )
    .unwrap();
}

fn over(root: &std::path::Path) -> Occupancy {
    Occupancy::over(
        Store::locate_in(root),
        Stores::at(Some(root), None, None),
    )
}

#[tokio::test]
async fn a_watchers_refresh_answers_until_the_watcher_ends() {
    let root = tempfile::tempdir().unwrap();
    let occupancy = over(root.path());
    rollout(root.path(), "first");
    let watching = occupancy.watching();
    let all: BTreeSet<Agent> = Agent::ALL.into_iter().collect();
    occupancy.refresh(&all).await.unwrap();

    rollout(root.path(), "second");
    assert_eq!(occupancy.sighted().await.unwrap().seen.len(), 1);

    drop(watching);
    assert_eq!(occupancy.sighted().await.unwrap().seen.len(), 2);
}

#[tokio::test]
async fn without_a_watcher_each_ask_reads_the_stores_again() {
    let root = tempfile::tempdir().unwrap();
    let occupancy = over(root.path());
    rollout(root.path(), "first");
    assert_eq!(occupancy.sighted().await.unwrap().seen.len(), 1);

    rollout(root.path(), "second");
    assert_eq!(occupancy.sighted().await.unwrap().seen.len(), 2);
}
