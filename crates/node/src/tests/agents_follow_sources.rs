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
