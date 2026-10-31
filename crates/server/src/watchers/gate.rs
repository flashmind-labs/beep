//! Which new stretches of a chat are worth a watcher's model call. Memory
//! reads every stretch. Skills only reads one where Beep did real work, and
//! improvement reads those too plus any where a call was denied or failed.

use crate::{
    permissions,
    store::{StoredBody, StoredMessage},
};

/// Tool calls a stretch needs before the skills or improvement watcher
/// reads it.
pub(super) const MIN_TOOL_CALLS: usize = 3;

/// Whether Beep made at least [`MIN_TOOL_CALLS`] tool calls in `messages`.
pub(super) fn busy(messages: &[StoredMessage]) -> bool {
    let calls = messages
        .iter()
        .filter(|message| matches!(message.body, StoredBody::Tool { .. }))
        .count();
    calls >= MIN_TOOL_CALLS
}

/// Whether a tool call in `messages` failed or was denied by the user.
pub(super) fn friction(messages: &[StoredMessage]) -> bool {
    messages.iter().any(|message| {
        matches!(
            &message.body,
            StoredBody::Tool { success, output, .. }
                if !success || output == permissions::DENIED
        )
    })
}

#[cfg(test)]
mod tests {
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
    fn a_lone_hello_is_neither_busy_nor_friction() {
        let messages = [
            user(1),
            message(
                2,
                StoredBody::Assistant {
                    content: "Hi!".into(),
                },
            ),
        ];
        assert!(!busy(&messages));
        assert!(!friction(&messages));
    }

    #[test]
    fn busy_takes_three_tool_calls() {
        let two = [user(1), tool(2, "ok", true), tool(3, "ok", true)];
        assert!(!busy(&two));
        let three = [
            user(1),
            tool(2, "ok", true),
            tool(3, "ok", true),
            tool(4, "ok", true),
        ];
        assert!(busy(&three));
        assert!(!friction(&three));
    }

    #[test]
    fn a_failed_or_denied_call_is_friction() {
        assert!(friction(&[user(1), tool(2, "timeout", false)]));
        assert!(friction(&[user(1), tool(2, permissions::DENIED, true)]));
        assert!(!friction(&[user(1), tool(2, "ok", true)]));
    }
}
