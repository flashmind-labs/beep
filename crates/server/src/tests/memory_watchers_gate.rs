use super::*;
use crate::store::TurnRecord;
use chrono::DateTime;
use uuid::Uuid;

fn message(id: i64, body: StoredBody) -> StoredMessage {
    StoredMessage {
        id,
        chat_id: Uuid::nil(),
        turn_id: None,
        sent_at: DateTime::UNIX_EPOCH,
        author_id: None,
        body,
        images: Vec::new(),
    }
}

fn user(id: i64) -> StoredMessage {
    message(
        id,
        StoredBody::User {
            content: "hello".into(),
            turn: TurnRecord::default(),
        },
    )
}

fn tool(id: i64, output: &str, success: bool) -> StoredMessage {
    message(
        id,
        StoredBody::Tool {
            call_id: format!("c{id}"),
            name: "web_search".into(),
            output: output.into(),
            success,
            description: String::new(),
        },
    )
}

#[test]
fn a_failed_or_denied_call_is_friction() {
    assert!(friction(&[user(1), tool(2, "timeout", false)]));
    assert!(friction(&[user(1), tool(2, permissions::DENIED, true)]));
    assert!(!friction(&[user(1), tool(2, "ok", true)]));
}
