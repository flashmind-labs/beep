use super::*;

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
    assert!(pending.record_paths(
        &roots,
        std::slice::from_ref(&agy),
        Some(&[empty_wal]),
    ));
    assert_eq!(pending.agents, BTreeSet::from([Agent::Agy]));

    pending.agents.clear();
    let rollout = codex.join("2026/11/27/rollout-1.jsonl");
    assert!(pending.record_paths(&roots, &[codex], Some(&[rollout])));
    assert_eq!(pending.agents, BTreeSet::from([Agent::Codex]));
}
