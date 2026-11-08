use super::*;

#[test]
fn only_a_rejected_token_is_dropped() {
    let violation = |field: &str| {
        json!({"error": {"code": 400, "details": [
            {"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError"},
            {"@type": "type.googleapis.com/google.rpc.BadRequest",
             "fieldViolations": [{"field": field, "description": "invalid"}]},
        ]}})
        .to_string()
    };
    assert!(rejects_token(&violation("message.token")));
    assert!(!rejects_token(&violation("message.data")));
    assert!(!rejects_token("not json"));
}
