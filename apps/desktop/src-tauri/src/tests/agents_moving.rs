use super::*;

#[test]
fn a_node_from_before_moves_says_to_update() {
    let view = MoveProgressView::from(MoveState::NeedsUpdate);
    assert_eq!((view.answer, view.phase), ("update", "waiting"));
}

#[test]
fn a_known_move_carries_its_numbers_and_reason() {
    let view = MoveProgressView::from(MoveState::Known(MoveProgress {
        phase: MovePhase::Failed,
        copied: 4,
        total: 6,
        files_left: 0,
        source_away: false,
        reason: Some("no checkout here".into()),
        worktree: Some("feat".into()),
        worktree_at: 1,
        worktrees: 2,
        edits_left: true,
        files_copied: 3,
        files_total: 9,
    }));
    assert_eq!(
        (
            view.answer,
            view.phase,
            view.worktree.as_deref(),
            view.worktree_at,
            view.worktrees
        ),
        ("known", "failed", Some("feat"), 1, 2)
    );
    assert_eq!(view.reason.as_deref(), Some("no checkout here"));
    assert!(view.edits_left);
}

#[test]
fn a_target_that_pushes_its_moves_answers_without_being_asked() {
    let to = iroh::SecretKey::from_bytes(&[4; 32]).public();
    let record = MoveView {
        session: SessionId::from("s1"),
        agent: "claude".into(),
        from: String::new(),
        to: to.to_string(),
        project: "p".into(),
        at: 7,
    };
    let entry = |at, phase| MoveEntry {
        session: SessionId::from("s1"),
        at,
        progress: MoveProgress {
            phase,
            ..MoveProgress::default()
        },
    };
    let mut pushed = HashMap::new();

    // Nothing pushed: the target is asked.
    assert!(pushed_progress(&record, &pushed).is_none());

    pushed.insert(to, vec![entry(6, MovePhase::Done)]);
    assert_eq!(pushed_progress(&record, &pushed).unwrap().answer, "unknown");

    pushed.insert(to, vec![entry(7, MovePhase::Files)]);
    let view = pushed_progress(&record, &pushed).unwrap();
    assert_eq!((view.answer, view.phase), ("known", "files"));
}
