use super::*;
use serde_json::json;

#[test]
fn arguments_are_bounded_at_a_utf8_boundary_and_marked_truncated() {
    let (text, truncated) = json(Some(&json!({"text": "é".repeat(MAX_DETAIL)})));
    assert!(truncated);
    assert!(text.unwrap().len() <= MAX_DETAIL);
    assert_eq!(json(None), (None, false));
    assert_eq!(json(Some(&Value::Null)), (None, false));
}
