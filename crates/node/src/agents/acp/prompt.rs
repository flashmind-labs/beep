//! Prompt contents, image validation and steering requests.

use crate::agents::picture::{Picture, image_line};
use agent_client_protocol::{
    UntypedMessage,
    v1::schema::{
        ContentBlock, ImageContent, InitializeResponse, PromptRequest, SessionId, TextContent,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{ApiError, Upload, check_agent_images};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// A message for the agent: its text, then any images.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(in crate::agents) struct Prompt {
    pub(in crate::agents) text: String,
    pub(in crate::agents) images: Vec<Upload>,
    /// Beep's word to the agent that goes with this message, ahead of the
    /// text and apart from it: the text stays what the user typed.
    #[serde(default)]
    pub(in crate::agents) context: Option<String>,
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
            context: None,
        }
    }
}

impl Prompt {
    /// How the transcript shows it: the text, then a line per image by its
    /// key, `[image: image-<key>]`, which the apps ask the node for.
    pub(in crate::agents) fn shown(&self) -> String {
        let mut shown = self.text.clone();

        for image in &self.images {
            if !shown.is_empty() {
                shown.push('\n');
            }
            shown.push_str(&image_line(&image.data));
        }

        shown
    }

    /// Its images, for the transcript's lines to show.
    pub(in crate::agents) fn pictures(&self) -> impl Iterator<Item = Picture> + '_ {
        self.images.iter().map(|image| Picture {
            mime: image.mime.clone(),
            data: image.data.clone(),
        })
    }

    /// A prompt with images, refused when there are too many, they are too
    /// large, or one is not the image it says it is.
    pub(in crate::agents) fn with_images(
        text: String,
        images: Vec<Upload>,
    ) -> Result<Self, String> {
        check_agent_images(&images).map_err(|e| match e {
            ApiError::BadRequest { message } => message,
            other => other.to_string(),
        })?;
        Ok(Self {
            text,
            images,
            context: None,
        })
    }

    /// The bytes it holds, to bound what waits in a queue.
    pub(in crate::agents) fn size(&self) -> usize {
        self.images.iter().fold(self.text.len(), |n, image| {
            n.saturating_add(image.data.len())
        })
    }
}

/// Whether the agent the `initialize` answer describes takes image blocks.
pub(in crate::agents) fn takes_images(initialized: &InitializeResponse) -> bool {
    initialized.agent_capabilities.prompt_capabilities.image
}

/// Only versions with the host-owned idle fallback can safely steer: older
/// adapters may start a detached turn when the running prompt just ended.
pub(in crate::agents) fn takes_steering(initialized: &InitializeResponse) -> bool {
    let advertised = initialized
        .meta
        .as_ref()
        .and_then(|meta| meta.get("steering"))
        .is_some_and(|steering| steering["supported"] == true);
    let Some(agent) = initialized.agent_info.as_ref().filter(|_| advertised) else {
        return false;
    };
    let minimum = match agent.name.as_str() {
        "@agentclientprotocol/codex-acp" => (2, 4, 0),
        "@agentclientprotocol/claude-agent-acp" => (0, 65, 0),
        "flsh" => (0, 2, 14),
        "pi-acp" => (0, 0, 34),
        super::bridge::CODEX_NAME => (0, 0, 0),
        _ => return false,
    };
    let mut parts = agent.version.split('.');
    let version = (|| {
        Some((
            parts.next()?.parse::<u32>().ok()?,
            parts.next()?.parse::<u32>().ok()?,
            parts.next()?.parse::<u32>().ok()?,
        ))
    })();
    parts.next().is_none() && version.is_some_and(|version| version >= minimum)
}

/// Inject into the running turn, leaving an idle session's input with us.
pub(in crate::agents) fn steering(
    session: &SessionId,
    input: &Prompt,
) -> Result<UntypedMessage, agent_client_protocol::Error> {
    let request = prompt(session, input, None);
    UntypedMessage::new(
        "_session/steering",
        json!({
            "sessionId": session,
            "prompt": request.prompt,
            "_meta": {"steering": {"idleBehavior": "promptRequired"}},
        }),
    )
}

#[derive(Debug, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub(in crate::agents) enum SteeringOutcome {
    Injected,
    PromptRequired,
    Failed,
}

/// `session/prompt` with `brief` and the prompt's own context (Beep's word to
/// the agent, each tagged) and the text first, then each image as an image block. Only images go when there is
/// no text.
pub(in crate::agents) fn prompt(
    session: &SessionId,
    prompt: &Prompt,
    brief: Option<&str>,
) -> PromptRequest {
    let mut blocks = Vec::with_capacity(3 + prompt.images.len());

    for context in brief.into_iter().chain(prompt.context.as_deref()) {
        blocks.push(ContentBlock::Text(TextContent::new(
            crate::agents::tags::context(context),
        )));
    }

    if !prompt.text.is_empty() || prompt.images.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(prompt.text.as_str())));
    }
    for image in &prompt.images {
        blocks.push(ContentBlock::Image(ImageContent::new(
            STANDARD.encode(&image.data),
            image.mime.as_str(),
        )));
    }

    PromptRequest::new(session.clone(), blocks)
}

#[cfg(test)]
#[path = "../../tests/agents_acp_prompt.rs"]
mod tests;
