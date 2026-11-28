use super::*;
use std::collections::BTreeSet;

#[test]
fn changes_keep_every_affected_harness_until_the_batch_is_taken() {
    let codex = PathBuf::from("/srv/codex");
    let claude = PathBuf::from("/srv/claude");
    let flsh = PathBuf::from("/srv/flsh");
    let roots = vec![
        (Agent::Codex, codex.clone()),
        (Agent::Claude, claude.clone()),
        (Agent::Flsh, flsh),
    ];
    let mut pending = Pending::default();
    pending.record(&roots, &[codex]);
    pending.record(&roots, &[claude]);
    let batch = std::mem::take(&mut pending);
    assert_eq!(batch.agents, BTreeSet::from([Agent::Codex, Agent::Claude]));
    assert!(pending.agents.is_empty());
}

#[test]
fn shared_watched_roots_invalidate_each_owner() {
    let root = PathBuf::from("/srv/sessions");
    let roots = vec![(Agent::Codex, root.clone()), (Agent::Pi, root.clone())];
    let mut pending = Pending::default();
    pending.record(&roots, &[root]);
    assert_eq!(pending.agents, BTreeSet::from([Agent::Codex, Agent::Pi]));
}

#[test]
fn record_paths_ignores_logs_shm_and_empty_wal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opencode = dir.path().join("opencode");
    let agy = dir.path().join("conversations");
    let codex = dir.path().join("sessions");
    std::fs::create_dir_all(opencode.join("log")).unwrap();
    std::fs::create_dir_all(&agy).unwrap();
    std::fs::create_dir_all(&codex).unwrap();

    let empty_wal = agy.join("a.db-wal");
    std::fs::File::create(&empty_wal).unwrap();
    let shm = agy.join("a.db-shm");
    std::fs::write(&shm, [0u8; 16]).unwrap();
    let opencode_log = opencode.join("log/opencode.log");
    std::fs::write(&opencode_log, "line\n").unwrap();

    let roots = vec![
        (Agent::OpenCode, opencode.clone()),
        (Agent::Agy, agy.clone()),
        (Agent::Codex, codex.clone()),
    ];
    let mut pending = Pending::default();

    assert!(!pending.record_paths(
        &roots,
        &[opencode.clone(), agy.clone()],
        Some(&[opencode_log, shm, empty_wal.clone()]),
    ));
    assert!(pending.agents.is_empty());

    std::fs::write(&empty_wal, "wal-frames").unwrap();
    assert!(pending.record_paths(&roots, std::slice::from_ref(&agy), Some(&[empty_wal]),));
    assert_eq!(pending.agents, BTreeSet::from([Agent::Agy]));

    pending.agents.clear();
    let rollout = codex.join("2026/11/27/rollout-1.jsonl");
    assert!(pending.record_paths(&roots, &[codex], Some(&[rollout])));
    assert_eq!(pending.agents, BTreeSet::from([Agent::Codex]));
}

/// A bridge built from its five parts and nothing else, over `root`.
#[cfg(unix)]
fn bridge_over(root: &std::path::Path) -> Arc<EventBridge> {
    use crate::agents::{Reopen, family::links::Links, reviews::kept::Reviews};

    let events = Hub::default();
    let sessions = Arc::new(Sessions::default());
    let occupancy = crate::occupancy::Occupancy::over(
        crate::logs::claude::Store::locate_in(root),
        crate::logs::found::Stores::at(Some(root), None, None),
    );
    let listings = Arc::new(Listings {
        sessions: sessions.clone(),
        occupancy: occupancy.clone(),
        images: Arc::default(),
        events: events.clone(),
        reviews: Arc::new(Reviews::new(events.clone(), sessions.clone())),
        reopen: Arc::new(Reopen {
            sessions: sessions.clone(),
            links: Arc::new(Links::new(Default::default())),
            ..Default::default()
        }),
        on_disk: Default::default(),
        rescanning: Default::default(),
        listed: Default::default(),
    });
    let reach = Arc::new(Reach {
        folders: Vec::new(),
        sessions: sessions.clone(),
        occupancy: occupancy.clone(),
    });

    Arc::new(EventBridge::new(
        events, occupancy, listings, reach, sessions,
    ))
}

#[cfg(unix)]
#[tokio::test]
async fn the_bridge_watches_without_agents_and_its_roots_end_with_the_last_follower() {
    let root = tempfile::tempdir().unwrap();
    let bridge = bridge_over(root.path());
    let lease = bridge.watch_event_sources().await.unwrap();
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", "read message"])
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    bridge.observe_external(&[beep_proto::node::agent::ExternalSession {
        id: "external-example".into(),
        agent: "codex".into(),
        cwd: root.path().to_string_lossy().into_owned(),
        pid: child.id(),
        ..Default::default()
    }]);

    assert_eq!(bridge.observed_work_roots(), [root.path().to_owned()]);
    assert_eq!(bridge.notification_roots().await.unwrap().len(), 1);

    drop(lease);

    assert!(bridge.observed_work_roots().is_empty());
    child.kill().await.unwrap();
}
