//! Images attached to a user message, described once by a vision model for models and turns that cannot see them.

use crate::attachments;
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
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
/// Largest data URI accepted per image.
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

/// A picture as it is kept: its type, bytes and size in pixels.
pub struct Picture {
    pub mime: String,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decodes the pictures in `uris`, skipping any that are not a readable image.
pub fn pictures(uris: &[String]) -> Vec<Picture> {
    uris.iter().filter_map(|uri| picture(uri)).collect()
}

fn picture(uri: &str) -> Option<Picture> {
    let ContentPart::Image { media_type, data } = parse_data_uri(uri)? else {
        return None;
    };
    let data = STANDARD.decode(data).ok()?;
    let size = imagesize::blob_size(&data).ok()?;
    Some(Picture {
        mime: media_type,
        width: u32::try_from(size.width).ok()?,
        height: u32::try_from(size.height).ok()?,
        data,
    })
}

/// The images as parts, each after its label when there are several.
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

/// The descriptions as text parts; an image that could not be described says so.
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
            let prompt = format!(
                "The user sent this image with the message: {message}\n\
                 Describe the whole image, not only what the message asks about."
            );
            match self.describe(image, prompt).await {
                Ok(text) => Some(text),
                Err(error) => {
                    log::warn!("unable to describe an image: {error}");
                    None
                }
            }
        }))
        .await
    }

    /// Describes the image file `name` a tool read.
    pub(crate) async fn describe_file(&self, image: &ContentPart, name: &str) -> Result<String> {
        let prompt = format!("This image is the file {name}. Describe the whole image.");
        self.describe(image, prompt).await
    }

    async fn describe(&self, image: &ContentPart, prompt: String) -> Result<String> {
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
            session_id: None,
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

/// Replaces the turn's images with their descriptions, keeping images whose description failed.
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
    let (images, files): (Vec<ContentPart>, Vec<ContentPart>) = parts
        .drain(..)
        .filter(|part| matches!(part, ContentPart::Image { .. }) || attachments::is_file(part))
        .partition(|part| matches!(part, ContentPart::Image { .. }));
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
    parts.extend(files);
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
    fn pictures_keep_their_size_and_skip_what_is_not_an_image() {
        // A 1x1 PNG.
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
        let kept = pictures(&[
            png.to_owned(),
            "data:image/png;base64,AAAA".to_owned(),
            "data:image/png;base64,not base64".to_owned(),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            (kept[0].mime.as_str(), kept[0].width, kept[0].height),
            ("image/png", 1, 1)
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

    #[test]
    fn files_sent_with_images_stay() {
        let started = Utc::now();
        let mut conversation = Conversation::new();
        let file = ContentPart::Text {
            text: "<file name=\"a.csv\" type=\"text/csv\" size=\"3 bytes\">\na,b\n</file>".into(),
        };
        let mut parts = labelled(&[image("a")]);
        parts.push(file.clone());
        conversation.add(ConversationEntry::user_with_parts("what is this?", parts));

        replace_with_descriptions(&mut conversation, started, &[Some("a cat".into())]);

        let EntryKind::User {
            parts: Some(parts), ..
        } = &conversation.entries()[0].kind
        else {
            panic!("the user message lost its parts");
        };
        assert_eq!(
            texts(parts),
            ["[Image 1: a cat]".to_owned(), texts(&[file])[0].clone()]
        );
    }
}
