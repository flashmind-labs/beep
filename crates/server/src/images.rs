//! Images attached to a user message.
//!
//! A model that sees images gets them as image parts, labelled "Image 1",
//! "Image 2" when there are several. Each image is also described once by a
//! vision model; a model without vision gets those descriptions instead, and
//! once the turn is saved they replace the images in the conversation so later
//! turns do not resend the image data.

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use flashmind::core::{Conversation, EntryKind};
use flashmind::types::{
    CompletionRequest, ContentPart, LlmProvider, Message, Model, ReasoningLevel, SamplingParams,
    StreamEvent,
};
use futures::StreamExt;
use futures::future::join_all;
use std::sync::Arc;

/// Most images one message may carry.
pub const MAX_IMAGES: usize = 8;
/// Largest data URI accepted per image. The phone sends JPEGs of at most
/// 1024 px, which stay well under this.
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;

const SYSTEM: &str = "You describe images for an assistant that cannot see them. \
First copy out any text in the image exactly, keeping numbers, names, prices and dates. \
Then describe what the image shows: objects, people, place, layout and anything notable. \
Be complete but brief, plain text, no preamble.";

/// Parses a `data:image/jpeg;base64,...` URI into an image part.
pub fn parse_data_uri(uri: &str) -> Option<ContentPart> {
    let rest = uri.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;
    let media_type = header.strip_suffix(";base64")?;
    if !media_type.starts_with("image/") {
        return None;
    }
    Some(ContentPart::Image {
        media_type: media_type.to_owned(),
        data: data.to_owned(),
    })
}

/// The images as parts for a model that sees them, each after its label when
/// there are several.
pub fn labelled(images: &[ContentPart]) -> Vec<ContentPart> {
    if images.len() < 2 {
        return images.to_vec();
    }
    images
        .iter()
        .enumerate()
        .flat_map(|(index, image)| {
            [
                ContentPart::Text {
                    text: format!("Image {}:", index + 1),
                },
                image.clone(),
            ]
        })
        .collect()
}

/// The descriptions as text parts, for a model without vision or for the saved
/// conversation. An image that could not be described says so.
pub fn described(descriptions: &[Option<String>]) -> Vec<ContentPart> {
    descriptions
        .iter()
        .enumerate()
        .map(|(index, description)| ContentPart::Text {
            text: match description {
                Some(text) => format!("[Image {}: {text}]", index + 1),
                None => format!("[Image {}: could not be read]", index + 1),
            },
        })
        .collect()
}

/// Describes images with a vision model.
#[derive(Clone)]
pub struct Describer {
    pub provider: Arc<dyn LlmProvider>,
    pub model: Model,
    pub user: String,
}

impl Describer {
    /// Describes every image at once, `None` for one that failed.
    pub async fn describe_all(&self, images: &[ContentPart], message: &str) -> Vec<Option<String>> {
        join_all(images.iter().map(|image| async move {
            match self.describe(image, message).await {
                Ok(text) => Some(text),
                Err(error) => {
                    log::warn!("unable to describe an image: {error}");
                    None
                }
            }
        }))
        .await
    }

    async fn describe(&self, image: &ContentPart, message: &str) -> Result<String> {
        let prompt = format!(
            "The user sent this image with the message: {message}\n\
             Describe the whole image, not only what the message asks about."
        );
        let request = CompletionRequest {
            model: self.model.clone(),
            messages: vec![
                Message::system(SYSTEM),
                Message::user_with_parts(prompt, vec![image.clone()]),
            ],
            tools: Vec::new(),
            max_tokens: Some(1500),
            reasoning: ReasoningLevel::Off,
            sampling: SamplingParams::default(),
            modalities: Vec::new(),
            audio_config: None,
            image_config: None,
            user: Some(self.user.clone()),
            provider_preferences: None,
        };
        let mut stream = self.provider.complete(request);
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::ContentDelta(delta)) => text.push_str(&delta),
                Ok(StreamEvent::Finished(_)) => break,
                Ok(_) => {}
                // Provider errors can echo upstream bodies; only the fact is kept.
                Err(_) => bail!("provider error"),
            }
        }
        let text = text.trim();
        if text.is_empty() {
            bail!("the vision model wrote nothing");
        }
        Ok(text.to_owned())
    }
}

/// Replaces the images of the turn's user message with their descriptions.
/// Images whose description failed stay, so the model can still look at them.
pub fn replace_with_descriptions(
    conversation: &mut Conversation,
    turn_started: DateTime<Utc>,
    descriptions: &[Option<String>],
) {
    let Some(entry) = conversation
        .entries_mut()
        .iter_mut()
        .find(|entry| entry.timestamp >= turn_started && entry.is_user())
    else {
        return;
    };
    let EntryKind::User {
        parts: Some(parts), ..
    } = &mut entry.kind
    else {
        return;
    };
    let images: Vec<ContentPart> = parts
        .drain(..)
        .filter(|part| matches!(part, ContentPart::Image { .. }))
        .collect();
    let several = images.len() > 1;
    for (index, image) in images.into_iter().enumerate() {
        match descriptions.get(index) {
            Some(Some(text)) => parts.push(ContentPart::Text {
                text: format!("[Image {}: {text}]", index + 1),
            }),
            _ => {
                if several {
                    parts.push(ContentPart::Text {
                        text: format!("Image {}:", index + 1),
                    });
                }
                parts.push(image);
            }
        }
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

    fn texts(parts: &[ContentPart]) -> Vec<String> {
        parts
            .iter()
            .map(|part| match part {
                ContentPart::Text { text } => text.clone(),
                ContentPart::Image { data, .. } => format!("<{data}>"),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn parses_only_image_data_uris() {
        assert!(matches!(
            parse_data_uri("data:image/png;base64,AAAA"),
            Some(ContentPart::Image { media_type, data }) if media_type == "image/png" && data == "AAAA"
        ));
        assert!(parse_data_uri("data:text/plain;base64,AAAA").is_none());
        assert!(parse_data_uri("https://example.com/a.png").is_none());
    }

    #[test]
    fn labels_only_several_images() {
        assert_eq!(texts(&labelled(&[image("a")])), ["<a>"]);
        assert_eq!(
            texts(&labelled(&[image("a"), image("b")])),
            ["Image 1:", "<a>", "Image 2:", "<b>"]
        );
    }

    #[test]
    fn swaps_described_images_and_keeps_failed_ones() {
        let started = Utc::now();
        let mut conversation = Conversation::new();
        conversation.add(ConversationEntry::user_with_parts(
            "which is cheaper?",
            labelled(&[image("a"), image("b"), image("c")]),
        ));
        let descriptions = [Some("receipt, 4 EUR".into()), None, Some("menu".into())];

        replace_with_descriptions(&mut conversation, started, &descriptions);

        let EntryKind::User {
            parts: Some(parts), ..
        } = &conversation.entries()[0].kind
        else {
            panic!("the user message lost its parts");
        };
        assert_eq!(
            texts(parts),
            [
                "[Image 1: receipt, 4 EUR]",
                "Image 2:",
                "<b>",
                "[Image 3: menu]"
            ]
        );
    }
}
