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

#[tokio::test]
async fn callers_asking_together_share_one_read() {
    let root = tempfile::tempdir().unwrap();
    let occupancy = over(root.path());
    let (first, second) = (occupancy.sighted(), occupancy.sighted());
    tokio::pin!(first, second);
    assert!(futures::poll!(first.as_mut()).is_pending());
    assert!(futures::poll!(second.as_mut()).is_pending());

    let running = occupancy.reading_now().unwrap();
    // This handle and the two callers'.
    assert_eq!(Arc::strong_count(&running), 3);
    drop(running);

    first.await.unwrap();
    second.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refresh_racing_the_last_watcher_leaves_nothing_published() {
    let root = tempfile::tempdir().unwrap();
    let occupancy = over(root.path());
    rollout(root.path(), "first");
    let all: BTreeSet<Agent> = Agent::ALL.into_iter().collect();

    for _ in 0..100 {
        let watching = occupancy.watching();
        let refreshing = {
            let (occupancy, all) = (occupancy.clone(), all.clone());
            tokio::spawn(async move { occupancy.refresh(&all).await.unwrap() })
        };
        drop(watching);
        refreshing.await.unwrap();

        assert!(
            beep_proto::watched::read_sent(&occupancy.0.reading.published, Option::is_none),
            "a copy outlived its last watcher"
        );
    }
}
