//! Images a tool hands the model, such as a node's screenshot.
//!
//! A tool result is text only, so a tool keeps its image in `ToolImages`
//! and writes `[image <id>]` in its output. Before each model call the
//! provider wrapper finds those marks in the tool results and adds the
//! images in a user message after them, the newest `ATTACHED` only, so a
//! long session of screenshots does not resend them all. The saved
//! conversation keeps just the mark.

use async_trait::async_trait;
use flashmind::types::{
    CompletionRequest, CompletionStream, ContentPart, ImageGenRequest, LlmProvider, Message, Model,
    ModelCapabilities, ModelInfo, Provider, Role, SttRequest, TtsRequest, VideoGenRequest, Voice,
};
use futures::{StreamExt, stream};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
};
use uuid::Uuid;

/// The most images one model call gets.
const ATTACHED: usize = 2;
/// The most images kept; older ones are forgotten.
const KEPT: usize = 32;
const MARK: &str = "[image ";

/// The images tools made lately, by id.
#[derive(Default)]
pub struct ToolImages(Mutex<VecDeque<(Uuid, ContentPart)>>);

impl ToolImages {
    /// Keeps `image` and returns the mark a tool result names it by.
    pub fn add(&self, image: ContentPart) -> String {
        let id = Uuid::new_v4();
        let mut images = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if images.len() >= KEPT {
            images.pop_front();
        }
        images.push_back((id, image));
        format!("{MARK}{id}]")
    }

    fn get(&self, id: &Uuid) -> Option<ContentPart> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(kept, _)| kept == id)
            .map(|(_, image)| image.clone())
    }
}

/// The ids of the marks in `text`, in order.
fn marks(text: &str) -> Vec<Uuid> {
    text.match_indices(MARK)
        .filter_map(|(at, _)| {
            let rest = &text[at + MARK.len()..];
            rest.split_once(']')?.0.parse().ok()
        })
        .collect()
}

/// An image a tool result marks.
struct Found {
    /// The message that marks it.
    index: usize,
    id: Uuid,
    image: ContentPart,
}

/// `messages` with the newest marked images added after the tool results
/// that name them. A model that cannot see images gets a note instead.
fn attach(messages: Vec<Message>, images: &ToolImages, sees: bool) -> Vec<Message> {
    let mut found: Vec<Found> = Vec::new();
    for (index, message) in messages.iter().enumerate().rev() {
        if found.len() >= ATTACHED {
            break;
        }
        if message.role != Role::Tool {
            continue;
        }
        for id in marks(&message.content).into_iter().rev() {
            if found.len() < ATTACHED
                && let Some(image) = images.get(&id)
            {
                found.push(Found { index, id, image });
            }
        }
    }
    if found.is_empty() {
        return messages;
    }
    // Grouped by the last tool result of the run each mark sits in, since
    // nothing may come between a call's results.
    let mut after: HashMap<usize, Vec<ContentPart>> = HashMap::new();
    for Found { index, id, image } in found.into_iter().rev() {
        let mut end = index;
        while messages
            .get(end + 1)
            .is_some_and(|next| next.role == Role::Tool)
        {
            end += 1;
        }
        let parts = after.entry(end).or_default();
        if sees {
            parts.push(ContentPart::Text {
                text: format!("{MARK}{id}]:"),
            });
            parts.push(image);
        } else {
            parts.push(ContentPart::Text {
                text: format!(
                    "{MARK}{id}] cannot be shown: this model does not see images. \
                     Read the screen with `desktop tree` instead."
                ),
            });
        }
    }
    let mut out = Vec::with_capacity(messages.len() + after.len());
    for (index, message) in messages.into_iter().enumerate() {
        out.push(message);
        if let Some(parts) = after.remove(&index) {
            out.push(Message::user_with_parts(
                "The images the tool results above refer to.",
                parts,
            ));
        }
    }
    out
}

/// A provider that adds tools' images to each request.
pub struct WithToolImages {
    inner: Arc<dyn LlmProvider>,
    images: Arc<ToolImages>,
}

impl WithToolImages {
    pub fn new(inner: Arc<dyn LlmProvider>, images: Arc<ToolImages>) -> Self {
        Self { inner, images }
    }
}

#[async_trait]
impl LlmProvider for WithToolImages {
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
        let marked = request
            .messages
            .iter()
            .any(|message| message.role == Role::Tool && message.content.contains(MARK));
        if !marked {
            return self.inner.complete(request);
        }
        let inner = self.inner.clone();
        let images = self.images.clone();
        Box::pin(
            stream::once(async move {
                let sees = inner.capabilities(&request.model).await.images;
                request.messages = attach(std::mem::take(&mut request.messages), &images, sees);
                inner.complete(request)
            })
            .flatten(),
        )
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

    fn image(data: &str) -> ContentPart {
        ContentPart::Image {
            media_type: "image/jpeg".into(),
            data: data.into(),
        }
    }

    fn images(parts: &[ContentPart]) -> Vec<String> {
        parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Image { data, .. } => Some(data.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn marks_are_found_in_text() {
        let kept = ToolImages::default();
        let mark = kept.add(image("a"));
        let id = marks(&format!("Screenshot, 1280x800. {mark}"));
        assert_eq!(id.len(), 1);
        assert!(kept.get(&id[0]).is_some());
        assert!(marks("[image not-an-id] and [image ").is_empty());
    }

    #[test]
    fn the_newest_images_follow_the_results_that_name_them() {
        let kept = ToolImages::default();
        let first = kept.add(image("first"));
        let second = kept.add(image("second"));
        let third = kept.add(image("third"));
        let messages = vec![
            Message::user("look at my screen"),
            Message::tool_result("1", format!("Screenshot. {first}")),
            Message::tool_result("2", format!("Screenshot. {second}")),
            Message::tool_result("3", "clicked"),
            Message::assistant("again"),
            Message::tool_result("4", format!("Screenshot. {third}")),
        ];
        let out = attach(messages, &kept, true);
        let roles: Vec<Role> = out.iter().map(|message| message.role).collect();
        assert_eq!(
            roles,
            [
                Role::User,
                Role::Tool,
                Role::Tool,
                Role::Tool,
                Role::User,
                Role::Assistant,
                Role::Tool,
                Role::User
            ],
            "images come after a run of results, never inside it"
        );
        assert_eq!(images(out[4].parts.as_deref().unwrap()), ["second"]);
        assert_eq!(images(out[7].parts.as_deref().unwrap()), ["third"]);
    }

    #[test]
    fn a_model_without_vision_gets_a_note() {
        let kept = ToolImages::default();
        let mark = kept.add(image("a"));
        let out = attach(vec![Message::tool_result("1", mark)], &kept, false);
        let parts = out[1].parts.as_deref().unwrap();
        assert!(images(parts).is_empty());
        assert!(
            matches!(&parts[0], ContentPart::Text { text } if text.contains("does not see images"))
        );
    }

    #[test]
    fn forgotten_images_are_left_as_text() {
        let kept = ToolImages::default();
        let first = kept.add(image("first"));
        for _ in 0..KEPT {
            kept.add(image("later"));
        }
        let out = attach(vec![Message::tool_result("1", first)], &kept, true);
        assert_eq!(out.len(), 1);
    }
}
