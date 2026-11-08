use super::*;

fn reply(json: &str) -> RepoReply {
    serde_json::from_str(json).expect("a repository reply")
}

#[test]
fn push_or_admin_is_write_access() {
    assert!(reply(r#"{"permissions":{"admin":false,"push":true,"pull":true}}"#).writable());
    assert!(reply(r#"{"permissions":{"admin":true,"push":false,"pull":true}}"#).writable());
}

#[test]
fn read_only_or_missing_permissions_are_not_write_access() {
    assert!(!reply(r#"{"permissions":{"admin":false,"push":false,"pull":true}}"#).writable());
    assert!(!reply(r#"{"full_name":"acme/widget"}"#).writable());
}
