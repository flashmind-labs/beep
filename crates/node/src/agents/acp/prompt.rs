//! Prompt contents, image validation and steering requests.

use crate::agents::images::{Picture, image_line};
use agent_client_protocol::{
    UntypedMessage,
    schema::v1::{
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
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
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

    /// A prompt with images, refused as the server refuses it: too many,
    /// too large, or not the image it says it is. The node checks again
    /// because a server is not the only thing that can reach it.
    pub(in crate::agents) fn with_images(
        text: String,
        images: Vec<Upload>,
    ) -> Result<Self, String> {
        check_agent_images(&images).map_err(|e| match e {
            ApiError::BadRequest { message } => message,
            other => other.to_string(),
        })?;
        Ok(Self { text, images })
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

/// `session/prompt` with `brief` (Beep's word to the agent, tagged) and the text
/// first, then each image as an image block. Only images go when there is
/// no text.
pub(in crate::agents) fn prompt(
    session: &SessionId,
    prompt: &Prompt,
    brief: Option<&str>,
) -> PromptRequest {
    let mut blocks = Vec::with_capacity(2 + prompt.images.len());
    if let Some(brief) = brief {
        blocks.push(ContentBlock::Text(TextContent::new(
            crate::agents::tags::context(brief),
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
mod tests {
    use super::*;

    fn png(name: &str, len: usize) -> Upload {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(len.max(data.len()), 0);
        Upload {
            name: name.into(),
            mime: "image/png".into(),
            data,
        }
    }

    #[test]
    fn the_node_refuses_images_the_server_would() {
        let text = || "look".to_owned();
        let ok = Prompt::with_images(text(), vec![png("a.png", 100)]).unwrap();
        assert_eq!(ok.images.len(), 1);

        let many = (0..=beep_proto::MAX_AGENT_IMAGES)
            .map(|i| png(&format!("{i}.png"), 10))
            .collect();
        let refused = Prompt::with_images(text(), many).unwrap_err();
        assert!(refused.starts_with("At most"), "{refused}");

        let big = png("big.png", beep_proto::MAX_AGENT_IMAGE + 1);
        let refused = Prompt::with_images(text(), vec![big]).unwrap_err();
        assert!(refused.starts_with("big.png is over"), "{refused}");

        let mut fake = png("fake.png", 10);
        fake.mime = "image/jpeg".into();
        let refused = Prompt::with_images(text(), vec![fake]).unwrap_err();
        assert!(refused.contains("not the image/jpeg"), "{refused}");

        let mut svg = png("x.svg", 10);
        svg.mime = "image/svg+xml".into();
        assert!(Prompt::with_images(text(), vec![svg]).is_err());
    }

    #[test]
    fn steering_requires_advertising_and_a_version_with_the_idle_fallback() {
        for (name, version, supported, expected) in [
            ("@agentclientprotocol/codex-acp", "2.4.0", true, true),
            ("@agentclientprotocol/codex-acp", "2.3.0", true, false),
            (
                "@agentclientprotocol/claude-agent-acp",
                "0.65.0",
                true,
                true,
            ),
            (
                "@agentclientprotocol/claude-agent-acp",
                "0.64.0",
                true,
                false,
            ),
            ("@agentclientprotocol/codex-acp", "2.4.0", false, false),
            (
                "@agentclientprotocol/codex-acp",
                "2.4.0-beta.1",
                true,
                false,
            ),
            ("other-agent", "3.0.0", true, false),
            ("flsh", "0.2.13", true, false),
            ("flsh", "0.2.14", true, true),
            ("flsh", "0.2.14", false, false),
        ] {
            let initialized = serde_json::from_value(json!({
                "protocolVersion": 1,
                "agentCapabilities": {},
                "agentInfo": {"name": name, "version": version},
                "_meta": {"steering": {"supported": supported}},
            }))
            .unwrap();
            assert_eq!(takes_steering(&initialized), expected, "{name} {version}");
        }
    }
}
