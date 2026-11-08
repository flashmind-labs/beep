//! Which new stretches of a chat are worth a watcher's model call.

use crate::{
    apps::permissions,
    store::{StoredBody, StoredMessage},
};

/// Tool calls a stretch needs before skills or improvement reads it.
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
#[path = "../../tests/memory_watchers_gate.rs"]
mod tests;
