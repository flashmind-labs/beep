//! Coding-agent sessions on the user's machines.

use crate::{AgentEffort, ApiError, MachineCapability, ModelId, Upload};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

/// How many sessions of one agent ran in a found folder.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoundAgent {
    /// Such as `claude`, `codex`, `pi`, `flsh` or `opencode`.
    pub agent: String,
    pub sessions: u32,
}

/// A project and its agent chat, as `GetProjectAgent` and `ListProjects`
/// return it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgent {
    pub project_id: Uuid,
    /// `org/repo`.
    pub name: String,
    pub chat_id: Uuid,
    pub open_tasks: u32,
    pub open_workers: u32,
}

/// An image sent to a coding agent, as `ListAgentImages` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentImage {
    /// As the session's log names it.
    pub name: String,
    /// The file to download.
    pub file_id: String,
    pub mime: String,
    /// [`image_key`] of its bytes: an agent that replays the session
    /// gives the image back without its name, and the node logs it as
    /// `[image: image-<key>]`.
    pub key: String,
}

/// One picture of a coding agent session's log, as `GetAgentImage` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentImageData {
    /// [`image_key`] of `data`.
    pub key: String,
    pub mime: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// Part of a media file on a machine, as `GetAgentFile` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFileChunk {
    /// The whole file's size in bytes.
    pub size: u64,
    pub mime: String,
    /// The bytes from the asked offset, at most [`AGENT_FILE_CHUNK`]; empty
    /// at or past the end.
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// The most of a file one `GetAgentFile` gives, well under a frame.
pub const AGENT_FILE_CHUNK: usize = 4 * 1024 * 1024;

/// The largest file `GetAgentFile` reads.
pub const MAX_AGENT_FILE: u64 = 128 * 1024 * 1024;

/// A machine that can run coding agents, as `ListAgentHosts` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentHost {
    /// The `Machine::id` it is.
    pub machine_id: String,
    pub machine_name: String,
    /// The agents it offers, such as `claude` and `codex`.
    pub agents: Vec<String>,
    /// The user's environment folder, when this machine is it.
    pub workspace: Option<String>,
    /// Its home folder, as it last reported it.
    pub home: Option<String>,
    pub capabilities: Vec<MachineCapability>,
}

/// The session `StartAgentSession` started.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartedAgent {
    pub session: MachineSession,
    /// Why it runs on that machine, such as "It has Xcode." Empty when the
    /// caller named the machine.
    pub why: String,
}

/// One session in full, as `GetAgentSession` reads it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSessionView {
    pub session: MachineSession,
    pub plan: Vec<AgentPlanStep>,
    /// Files it changed, newest last.
    pub files: Vec<String>,
    /// Its last message, cut short.
    pub last_message: String,
    /// Why it failed or its last turn went wrong.
    pub error: Option<String>,
    /// Messages waiting for the running turn to end.
    pub queued: u32,
    pub turns: u32,
    pub entries: Vec<AgentEntry>,
    /// Transcript entries before `entries`.
    pub earlier: u32,
}

/// What a session Beep runs has used, as `GetAgentSessionUsage` gives it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentSessionUsage {
    /// How full its context window is, as the agent last said; `None`
    /// before it says and right after it compacts.
    pub context: Option<AgentContext>,
    /// Every token its finished turns used.
    pub tokens: u64,
    /// What its finished turns cost at API prices, in US dollars; `None`
    /// for an agent that reports no cost, as Codex and Gemini do.
    pub cost_usd: Option<f64>,
}

/// How full a coding agent's context window is.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentContext {
    /// Tokens in the context.
    pub used: u64,
    /// The window's size in tokens.
    pub size: u64,
}

/// One step of a session's plan.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentPlanStep {
    pub content: String,
    pub status: AgentPlanStatus,
}

wire_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentPlanStatus {
        0 Pending {}
        1 InProgress {}
        2 Completed {}
    }
}

/// One entry of a session's transcript.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEntry {
    pub kind: AgentEntryKind,
    pub text: String,
}

wire_enum! {
    /// Who an entry is from: a prompt, the agent's text, a tool call, a
    /// permission question and its answer, or what the agent thought and
    /// for how many milliseconds (`None` while it still thinks, or when
    /// that is not known).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentEntryKind {
        0 User {}
        1 Agent {}
        2 Tool {}
        3 Question {}
        4 Other {}
        5 Thought { ms: Option<u64> } since THOUGHTS
    }
}

wire_enum! {
    /// What `SteerAgentSession` does to a session: `Prompt` sends it a
    /// message after the running turn, `Cancel` stops the turn and drops
    /// waiting messages, `Close` ends the session and stops the agent.
    /// `PromptImages` is `Prompt` with images after the text, as
    /// `check_agent_images` allows them; `text` may then be empty. A
    /// server older than `Feature::AGENT_IMAGES` refuses it, and so does an
    /// agent that takes no images. `Configure` switches the running session
    /// to `model` and thinking `effort` (either may be `None` to keep it),
    /// from its next turn; a server older than `Feature::AGENT_CONFIGURE`
    /// refuses it, and so does a node or agent that cannot switch.
    /// `Steer` is `PromptImages` for the running turn: the node delivers it
    /// once the agent's current tool call finishes rather than after the
    /// whole turn, and like `Prompt` when no turn runs. A server older than
    /// `Feature::AGENT_STEER` refuses it. `DropSteers` takes back the steers
    /// not yet delivered.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentSteer {
        0 Prompt { text: String }
        1 Cancel {}
        2 Close {}
        3 PromptImages { text: String, images: Vec<Upload> } since AGENT_IMAGES
        4 Configure { model: Option<ModelId>, effort: Option<AgentEffort> } since AGENT_CONFIGURE
        5 Steer { text: String, images: Vec<Upload> } since AGENT_STEER
        6 DropSteers {} since AGENT_STEER
    }
}

/// The image types `AgentSteer::PromptImages` may carry.
pub const AGENT_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The most images one `AgentSteer::PromptImages` may carry.
pub const MAX_AGENT_IMAGES: usize = 5;

/// The largest one image may be.
pub const MAX_AGENT_IMAGE: usize = 5 * 1024 * 1024;

/// The most all of a message's images may hold, so the request stays
/// under a frame.
pub const MAX_AGENT_IMAGES_TOTAL: usize = 7 * 1024 * 1024;

/// Refuses images an agent prompt may not carry: too many, too large, or
/// not a PNG, JPEG, GIF or WebP whose bytes say so too.
pub fn check_agent_images(images: &[Upload]) -> Result<(), ApiError> {
    if images.len() > MAX_AGENT_IMAGES {
        return Err(ApiError::bad_request(format!(
            "At most {MAX_AGENT_IMAGES} images go with one message."
        )));
    }
    let mut total = 0usize;
    for image in images {
        let name = if image.name.is_empty() {
            "An image"
        } else {
            image.name.as_str()
        };
        if !AGENT_IMAGE_TYPES.contains(&image.mime.as_str()) {
            return Err(ApiError::bad_request(format!(
                "{name} is not a PNG, JPEG, GIF or WebP image."
            )));
        }
        if image.data.len() > MAX_AGENT_IMAGE {
            return Err(ApiError::bad_request(format!(
                "{name} is over {} MB; images up to {} MB go.",
                MAX_AGENT_IMAGE >> 20,
                MAX_AGENT_IMAGE >> 20
            )));
        }
        if sniff_image(&image.data) != Some(image.mime.as_str()) {
            return Err(ApiError::bad_request(format!(
                "{name} is not the {} it says it is.",
                image.mime
            )));
        }
        total = total.saturating_add(image.data.len());
    }
    if total > MAX_AGENT_IMAGES_TOTAL {
        return Err(ApiError::bad_request(format!(
            "The images are over {} MB together.",
            MAX_AGENT_IMAGES_TOTAL >> 20
        )));
    }
    Ok(())
}

/// The image type `data` starts like, of `AGENT_IMAGE_TYPES`.
pub fn sniff_image(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// A coding-agent session on one of the user's machines, as
/// `ListAgentSessions` lists it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineSession {
    /// The `Machine::id` it runs on.
    pub machine_id: String,
    pub machine_name: String,
    /// The node's id for it, or the agent's own for an external session.
    pub id: String,
    /// The agent, such as `claude` or `codex`.
    pub agent: String,
    pub title: Option<String>,
    pub cwd: String,
    pub status: AgentStatus,
    /// What the agent is doing now, such as "Edit src/main.rs".
    pub doing: Option<String>,
    pub started_at: SystemTime,
    pub last_activity: SystemTime,
    /// Started outside Beep, such as Claude Code in a terminal.
    pub external: bool,
    /// What it waits on an answer to; never set for an external session.
    pub question: Option<AgentQuestion>,
}

wire_enum! {
    /// What a session is doing. `Waiting` and `Running` are only for
    /// external sessions: waiting for the user, or running with nothing
    /// more known.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum AgentStatus {
        0 Starting {}
        1 Working {}
        2 Asking {}
        3 Idle {}
        4 Ended {}
        5 Waiting {}
        6 Running {}
    }
}

/// A permission question a session asks before it acts.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentQuestion {
    pub id: u32,
    /// What the agent wants to do, such as "Run cargo test".
    pub text: String,
    /// The command, path or other input it would act on, cut short.
    pub detail: Option<String>,
    pub options: Vec<AgentOption>,
}

/// One answer to an `AgentQuestion`.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentOption {
    pub id: String,
    pub name: String,
    /// It lets the agent go ahead, once or always.
    pub allow: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(name: &str, mime: &str, data: Vec<u8>) -> Upload {
        Upload {
            name: name.into(),
            mime: mime.into(),
            data,
        }
    }

    fn png(size: usize) -> Vec<u8> {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(size.max(data.len()), 0);
        data
    }

    #[test]
    fn agent_images_of_the_four_types_pass() {
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.push(0);
        let images = [
            image("a.png", "image/png", png(100)),
            image("b.jpg", "image/jpeg", vec![0xff, 0xd8, 0xff, 0xe0]),
            image("c.gif", "image/gif", b"GIF89a..".to_vec()),
            image("d.webp", "image/webp", webp),
        ];
        assert_eq!(check_agent_images(&images), Ok(()));
        assert_eq!(check_agent_images(&[]), Ok(()));
    }

    #[test]
    fn agent_images_over_the_limits_are_refused() {
        let six: Vec<Upload> = (0..6)
            .map(|_| image("a.png", "image/png", png(10)))
            .collect();
        let error = check_agent_images(&six).unwrap_err().to_string();
        assert!(error.contains("At most 5 images"), "{error}");

        let big = [image("big.png", "image/png", png(MAX_AGENT_IMAGE + 1))];
        let error = check_agent_images(&big).unwrap_err().to_string();
        assert!(error.contains("big.png is over 5 MB"), "{error}");
        let at_limit = [image("ok.png", "image/png", png(MAX_AGENT_IMAGE))];
        assert_eq!(check_agent_images(&at_limit), Ok(()));

        let two = [
            image("a.png", "image/png", png(4 << 20)),
            image("b.png", "image/png", png(4 << 20)),
        ];
        let error = check_agent_images(&two).unwrap_err().to_string();
        assert!(error.contains("over 7 MB together"), "{error}");
    }

    #[test]
    fn agent_images_of_other_types_are_refused() {
        let svg = [image("a.svg", "image/svg+xml", b"<svg/>".to_vec())];
        let error = check_agent_images(&svg).unwrap_err().to_string();
        assert!(error.contains("a.svg is not a PNG"), "{error}");
        let lying = [image("a.png", "image/png", vec![0xff, 0xd8, 0xff])];
        let error = check_agent_images(&lying).unwrap_err().to_string();
        assert!(error.contains("not the image/png it says"), "{error}");
    }
}
