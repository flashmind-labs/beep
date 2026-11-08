use super::*;

#[test]
fn a_recent_write_with_nothing_holding_it_is_stopped() {
    assert_eq!(
        settle(Presence::Recent, None, Some(false)),
        Presence::Stopped
    );
    assert_eq!(settle(Presence::Recent, None, Some(true)), Presence::Recent);
    // When nothing can say, a recent write still counts.
    assert_eq!(settle(Presence::Recent, None, None), Presence::Recent);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_file_this_process_holds_is_held() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(&path, "{}\n").unwrap();
    let other = dir.path().join("other.jsonl");
    std::fs::write(&other, "{}\n").unwrap();
    let _open = std::fs::File::open(&path).unwrap();
    // A path nothing opened beside it. (A file closed again may still be
    // held for a moment by a child another test forks before it execs.)
    let held = held_open(vec![path.clone(), other]).await;
    assert_eq!(held, Some(HashMap::from([(path, std::process::id())])));
}
