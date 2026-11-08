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
        files_left: 12,
        source_away: false,
        reason: Some("no checkout here".into()),
    }));
    assert_eq!(
        (view.answer, view.phase, view.files_left),
        ("known", "failed", 12)
    );
    assert_eq!(view.reason.as_deref(), Some("no checkout here"));
}
