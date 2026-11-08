use super::*;

#[test]
fn a_session_is_continued_by_one_request_at_a_time() {
    let moves = Moves::default();
    let first = moves.begin_continue("s1").expect("the first goes");
    assert!(
        moves.begin_continue("s1").is_none(),
        "the second is refused"
    );
    assert!(
        moves.begin_continue("s2").is_some(),
        "another session is free"
    );
    drop(first);
    assert!(
        moves.begin_continue("s1").is_some(),
        "free again once it ended"
    );
}
