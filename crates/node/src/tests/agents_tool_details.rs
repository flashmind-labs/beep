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

#[test]
fn bounded_details_match_the_whole_text_cut() {
    let values = [
        json!("Finished waiting"),
        json!({"command": "cargo test", "n": 3}),
        json!("x".repeat(MAX_DETAIL - 2)),
        json!("x".repeat(MAX_DETAIL - 1)),
        json!("tab\there\n\"quoted\"".repeat(MAX_DETAIL / 4)),
        json!(["é".repeat(MAX_DETAIL / 2), "ü"]),
        json!([{"type": "text", "text": "€".repeat(MAX_DETAIL)}]),
    ];

    for value in values {
        let whole = value.to_string();
        let expected = (Some(cut(&whole, MAX_DETAIL)), whole.len() > MAX_DETAIL);
        let start: String = whole.chars().take(40).collect();
        assert_eq!(json(Some(&value)), expected, "{start}");
    }
}
