//! Files tools hand the model: only the newest `SHOWN` are sent, and none are saved.

use super::forward::forward_llm;
use flashmind::{
    core::{Conversation, EntryKind},
    types::{
        CompletionRequest, CompletionStream, ContentPart, LlmProvider, Message, ModelInfo, Role,
    },
};
use std::sync::Arc;

/// The most tool attachments one model call gets.
const SHOWN: usize = 3;
/// The text of the message flashmind puts tool result parts in.
const FROM_TOOLS: &str = "Attachments returned by the preceding tools:";
/// Added to a saved tool result whose attachments were dropped.
const DROPPED: &str = "[The attachment is no longer shown. Attach it again, or take a new \
    screenshot, to look at it.]";

/// What an attachment left out of a model call says in its place.
fn left_out(part: &ContentPart) -> String {
    let what = match part {
        ContentPart::Document { filename, .. }
        | ContentPart::Video { filename, .. }
        | ContentPart::Audio { filename, .. } => filename.as_str(),
        _ => "an image",
    };
    format!(
        "[{what} is no longer shown, to save space. Attach it again, or take a new screenshot, \
         to look at it.]"
    )
}

/// Whether `messages[index]` carries tool result parts.
fn carries_tool_parts(messages: &[Message], index: usize) -> bool {
    index > 0
        && messages[index - 1].role == Role::Tool
        && messages[index].role == Role::User
        && messages[index].content == FROM_TOOLS
}

/// Replaces all but the newest `SHOWN` tool attachments with a note.
fn keep_recent(messages: &mut [Message]) {
    let mut shown = 0;
    for index in (0..messages.len()).rev() {
        if !carries_tool_parts(messages, index) {
            continue;
        }
        let Some(parts) = messages[index].parts.as_mut() else {
            continue;
        };
        for part in parts.iter_mut().rev() {
            if matches!(part, ContentPart::Text { .. }) {
                continue;
            }
            if shown < SHOWN {
                shown += 1;
            } else {
                *part = ContentPart::Text {
                    text: left_out(part),
                };
            }
        }
    }
}

/// Drops every tool result's attachments, noting it in the output.
pub(crate) fn drop_attachments(conversation: &mut Conversation) {
    for entry in conversation.entries_mut() {
        if let EntryKind::Tool { output, parts, .. } = &mut entry.kind
            && !parts.is_empty()
        {
            parts.clear();
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(DROPPED);
        }
    }
}

/// A provider that sends only the newest tool attachments with each request.
pub struct RecentAttachments {
    inner: Arc<dyn LlmProvider>,
}

impl RecentAttachments {
    pub fn new(inner: Arc<dyn LlmProvider>) -> Self {
        Self { inner }
    }
}

forward_llm!(RecentAttachments {
    async fn list_models(&self) -> Option<Vec<ModelInfo>> {
        self.inner.list_models().await
    }

    fn complete(&self, mut request: CompletionRequest) -> CompletionStream {
        keep_recent(&mut request.messages);
        self.inner.complete(request)
    }
});

#[cfg(test)]
#[path = "../tests/agent_tool_attachments.rs"]
mod tests;
