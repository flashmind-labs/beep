use super::*;

#[test]
fn an_answer_without_a_turn_takes_the_turn_started_before_it() {
    let mut seen = Some("turn-1".to_owned());
    assert_eq!(
        started_turn(&json!({}), &mut seen),
        Some("turn-1".to_owned())
    );
    assert_eq!(seen, None);

    let mut seen = Some("turn-1".to_owned());
    assert_eq!(
        started_turn(&json!({"turn": {"id": "turn-2"}}), &mut seen),
        Some("turn-2".to_owned())
    );
}
