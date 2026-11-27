use beep_proto::SessionId;
use super::*;
use beep_proto::node::agent::Presence;
use serde_json::json;
use uuid::Uuid;

fn registration(id: &str) -> Registration {
    serde_json::from_value(json!({
        "version": 1, "agent": "flsh", "session_id": id,
        "cwd": std::env::temp_dir().join("presence-project"), "pid": 123
    }))
    .unwrap()
}

#[test]
fn only_the_last_connection_closes_a_session() {
    let registry = Registry::default();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    registry.register(registration("session_a"), a).unwrap();
    registry.register(registration("session_a"), b).unwrap();
    registry.close(&SessionId::from("session_a"), a);
    assert_eq!(registry.snapshot()[0].presence, Presence::Running);
    registry.close(&SessionId::from("session_a"), b);
    assert_eq!(registry.snapshot()[0].presence, Presence::Stopped);
    assert_eq!(registry.snapshot()[0].pid, None);
}

#[test]
fn registration_is_validated_and_session_metadata_updates() {
    let registry = Registry::default();
    let connection = Uuid::new_v4();
    let mut invalid = registration("session_a");
    invalid.cwd = "relative".into();
    assert!(registry.register(invalid, connection).is_err());
    assert!(registry.snapshot().is_empty());
    let mut changed = registration("session_a");
    let cwd = std::env::temp_dir()
        .join("presence-other")
        .display()
        .to_string();
    changed.cwd = cwd.clone();
    changed.title = Some("Working title".into());
    registry.register(changed, connection).unwrap();
    assert_eq!(registry.snapshot()[0].cwd, cwd);
    assert_eq!(
        registry.snapshot()[0].title.as_deref(),
        Some("Working title")
    );
}

