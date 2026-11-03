//! Files tools hand the model, such as a machine's screenshot or a file
//! `attach` read.
//!
//! A tool returns them as parts of its result, and flashmind's conversation
//! sends them in a user message after the tool results. Each model call
//! gets only the newest `SHOWN` of them, so a long run of screenshots does
//! not resend every one. When the turn is saved they are dropped, leaving a
//! note in the tool's output, so the saved conversation keeps no file data.

use async_trait::async_trait;
use flashmind::core::{Conversation, EntryKind};
use flashmind::types::{
    CompletionRequest, CompletionStream, ContentPart, ImageGenRequest, LlmProvider, Message, Model,
    ModelCapabilities, ModelInfo, Provider, Role, SttRequest, TtsRequest, VideoGenRequest, Voice,
};
use std::{collections::HashMap, sync::Arc};

/// The most tool attachments one model call gets.
const SHOWN: usize = 3;
/// The text of the user message flashmind's conversation puts tool
/// results' parts in.
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

/// Whether `messages[index]` is the message flashmind adds after tool
/// results to carry their parts.
fn carries_tool_parts(messages: &[Message], index: usize) -> bool {
    index > 0
        && messages[index - 1].role == Role::Tool
        && messages[index].role == Role::User
        && messages[index].content == FROM_TOOLS
}

/// Replaces all but the newest `SHOWN` tool attachments in `messages` with
/// a note saying they are left out.
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

/// Drops the attachments of every tool result in `conversation`, noting it
/// in the result's output, so the saved conversation holds no file data.
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

#[async_trait]
impl LlmProvider for RecentAttachments {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn provider(&self) -> Provider {
        self.inner.provider()
    }

    async fn context_window(&self, model: &Model) -> Option<u32> {
        self.inner.context_window(model).await
    }

    async fn capabilities(&self, model: &Model) -> ModelCapabilities {
        self.inner.capabilities(model).await
    }

    async fn list_models(&self) -> Option<Vec<ModelInfo>> {
        self.inner.list_models().await
    }

    fn complete(&self, mut request: CompletionRequest) -> CompletionStream {
        keep_recent(&mut request.messages);
        self.inner.complete(request)
    }

    fn text_to_speech(&self, request: TtsRequest) -> CompletionStream {
        self.inner.text_to_speech(request)
    }

    fn transcribe(&self, request: SttRequest) -> CompletionStream {
        self.inner.transcribe(request)
    }

    async fn list_voices(&self, model: &str) -> Option<Vec<Voice>> {
        self.inner.list_voices(model).await
    }

    fn generate_image(&self, request: ImageGenRequest) -> CompletionStream {
        self.inner.generate_image(request)
    }

    fn generate_video(&self, request: VideoGenRequest) -> CompletionStream {
        self.inner.generate_video(request)
    }

    fn update_routing(&self, routing: &HashMap<String, String>) {
        self.inner.update_routing(routing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flashmind::core::ConversationEntry;

    fn image(data: &str) -> ContentPart {
        ContentPart::Image {
            media_type: "image/jpeg".into(),
            data: data.into(),
        }
    }

    /// The data of every attachment the model gets, oldest first.
    fn sent(messages: &[Message]) -> Vec<String> {
        messages
            .iter()
            .flat_map(|message| message.parts.iter().flatten())
            .filter_map(|part| match part {
                ContentPart::Image { data, .. } | ContentPart::Document { data, .. } => {
                    Some(data.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn screenshots(count: usize) -> Conversation {
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user("look at my screen"));
        for n in 0..count {
            conversation.add(ConversationEntry::assistant("again"));
            conversation.add(ConversationEntry::tool_with_parts(
                n.to_string(),
                "Screenshot of mac",
                vec![image(&format!("shot {n}"))],
            ));
        }
        conversation
    }

    #[test]
    fn a_tool_attachment_reaches_the_model_in_flashminds_message() {
        let mut messages = screenshots(1).to_messages();
        keep_recent(&mut messages);
        let last = messages.last().expect("a message");
        assert_eq!(last.role, Role::User);
        assert_eq!(last.content, FROM_TOOLS, "flashmind changed its text");
        assert_eq!(sent(&messages), ["shot 0"]);
    }

    #[test]
    fn only_the_newest_attachments_are_sent() {
        let mut messages = screenshots(6).to_messages();
        keep_recent(&mut messages);
        assert_eq!(sent(&messages), ["shot 3", "shot 4", "shot 5"]);
        let notes = messages
            .iter()
            .flat_map(|message| message.parts.iter().flatten())
            .filter(|part| matches!(part, ContentPart::Text { text } if text.contains("no longer shown")))
            .count();
        assert_eq!(notes, 3);
    }

    #[test]
    fn a_saved_turn_keeps_no_attachment_data() {
        let mut conversation = screenshots(2);
        conversation.add(ConversationEntry::tool_with_parts(
            "pdf",
            "Attached report.pdf",
            vec![ContentPart::Document {
                media_type: "application/pdf".into(),
                filename: "report.pdf".into(),
                data: "cGRm".into(),
            }],
        ));
        drop_attachments(&mut conversation);
        let saved = serde_json::to_string(&conversation).expect("it serializes");
        assert!(
            !saved.contains("shot 0") && !saved.contains("cGRm"),
            "{saved}"
        );
        assert!(sent(&conversation.to_messages()).is_empty());
        let EntryKind::Tool { output, .. } = &conversation.entries()[2].kind else {
            panic!("a tool result");
        };
        assert!(output.starts_with("Screenshot of mac\n[The attachment is no longer shown"));
    }
}
