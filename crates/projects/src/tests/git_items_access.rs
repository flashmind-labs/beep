use super::*;
use secrecy::SecretString;

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

#[test]
fn a_kept_answer_is_dropped_when_the_tokens_change() {
    let repo = Repo {
        host: "git.example".into(),
        name: "acme/token-change".into(),
    };
    let service = Service::GitHub;
    let old = SecretString::from("old-token");
    let new = SecretString::from("new-token");
    let login = |secret| Login {
        service: &service,
        secret,
        account: None,
    };
    let before = fingerprint(&[login(&old)]);
    let after = fingerprint(&[login(&old), login(&new)]);
    remember(repo.clone(), before, false);
    assert_eq!(known(&repo, before), Some(false));
    assert_eq!(known(&repo, after), None);
    assert_eq!(fingerprint(&[login(&new), login(&old)]), after);
}
