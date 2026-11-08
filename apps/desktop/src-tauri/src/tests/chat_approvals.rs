use super::ApprovalAnswered;

#[test]
fn approval_answered_names_its_keys() {
    let payload = serde_json::to_value(ApprovalAnswered {
        session: "chat".into(),
        query_id: "query".into(),
    })
    .expect("a plain struct serializes");
    assert_eq!(
        payload,
        serde_json::json!({ "session": "chat", "query_id": "query" })
    );
}
