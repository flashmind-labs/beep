//! Coding-agent sessions on the user's machines.

use crate::SessionId;
use crate::{
    AccountName, ApiError, MachineCapability, ModelId, ProviderCredits, Upload,
    node::agent::{
        CheckoutPlan, Effort, LineChanges, Persona, SessionDevice, SessionInfo, StopReason,
        WorkPlace,
    },
};
use std::time::SystemTime;

wire_structs! {
    /// How many sessions of one agent ran in a found folder.
    pub struct FoundAgent {
        /// Such as `claude`, `codex`, `pi`, `flsh` or `opencode`.
        pub agent: String,
        pub sessions: u32,
    }

    /// One picture of a coding agent session's log, as `GetAgentImage` gives it.
    #[derive(Eq)]
    pub struct AgentImageData {
        /// [`image_key`] of `data`.
        pub key: String,
        pub mime: String,
        #[serde(with = "serde_bytes")]
        pub data: Vec<u8>,
    }

    /// Part of a media file on a machine, as `GetAgentFile` gives it.
    #[derive(Eq)]
    pub struct AgentFileChunk {
        /// The whole file's size in bytes.
        pub size: u64,
        pub mime: String,
        /// The bytes from the asked offset, at most [`AGENT_FILE_CHUNK`]; empty
        /// at or past the end.
        #[serde(with = "serde_bytes")]
        pub data: Vec<u8>,
    }
}

/// The most of a file one `GetAgentFile` gives, well under a frame.
pub const AGENT_FILE_CHUNK: usize = 4 * 1024 * 1024;

/// A machine that can run coding agents.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq)]
pub struct StartedAgent {
    pub session: MachineSession,
    /// Why it runs on that machine, such as "It has Xcode." Empty when the
    /// caller named the machine.
    pub why: String,
}

/// One session in full, as `GetAgentSession` reads it.
#[derive(Debug, Clone, PartialEq)]
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
    /// Text of messages waiting to be delivered.
    pub queued_messages: Vec<String>,
    /// The model reported by the session.
    pub model: Option<String>,
    /// Why its last turn ended; none before the first.
    pub last_stop: Option<StopReason>,
    /// The latest usage of a session Beep runs.
    pub usage: Option<AgentSessionUsage>,
}

wire_structs! {
    /// What a session Beep runs has used.
    #[derive(Default)]
    pub struct AgentSessionUsage {
        /// How full its context window is, as the agent last said; `None`
        /// before it says and right after it compacts.
        pub context: Option<AgentContext>,
        /// Every token its finished turns used.
        pub tokens: u64,
        /// What it cost so far at API prices, in US dollars; `None`
        /// for an agent that reports no cost, as Codex does.
        pub cost_usd: Option<f64>,
        /// Provider balances as this session last reported them.
        #[serde(default)]
        pub credits: Vec<ProviderCredits>,
    }

    /// The modes a session Beep runs can be switched between, as
    /// `GetAgentSessionModes` gives them. Empty from a node too old to say.
    #[derive(Default)]
    pub struct AgentSessionModes {
        /// The id of the mode it is in, once the agent said.
        pub current: Option<String>,
        /// In the order a switch goes through them.
        pub modes: Vec<AgentMode>,
        /// The plan waiting for approval in the session's question, in Markdown.
        pub plan: Option<String>,
    }

    /// The commands a session Beep runs has in the background, as
    /// `GetAgentSessionTasks` gives them. Empty from a node too old to say.
    #[derive(Default)]
    pub struct AgentSessionTasks {
        /// Oldest first; one stays after it ended.
        pub tasks: Vec<AgentTask>,
    }

    /// A command a coding agent left running in the background.
    pub struct AgentTask {
        /// The agent's own id, which `AgentSteer::StopTask` names.
        pub id: String,
        pub name: String,
        pub description: String,
        /// Such as `running` or `stopped`.
        pub state: String,
        pub can_stop: bool,
        /// The file its output goes to, on the machine.
        pub output_file: Option<String>,
        /// How it ended, when the agent says.
        pub summary: Option<String>,
        /// The transcript entry of the tool call that started it, counted
        /// from the start of the session, as `AgentSessionView::earlier`
        /// counts.
        pub entry: Option<u64>,
    }

    /// One mode of a coding-agent session.
    pub struct AgentMode {
        pub id: String,
        pub name: String,
    }

    /// How full a coding agent's context window is.
    #[derive(Copy, Default, Eq)]
    pub struct AgentContext {
        /// Tokens in the context.
        pub used: u64,
        /// The window's size in tokens.
        pub size: u64,
    }
}

/// One step of a session's plan.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AgentPlanStep {
    pub content: String,
    pub status: AgentPlanStatus,
    /// Unix seconds when the step first ran, and when it stopped.
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AgentPlanStatus {
    Pending {},
    InProgress {},
    Completed {},
}

impl serde::Serialize for AgentPlanStatus {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(Into::<&'static str>::into(*self))
    }
}

wire_structs! {
    /// One entry of a session's transcript.
    pub struct AgentEntry {
        pub kind: AgentEntryKind,
        pub text: String,
        #[serde(default)]
        pub tool: Option<AgentToolCall>,
        #[serde(default)]
        pub sent_at: Option<u64>,
    }

    /// The actual tool call, kept separately from its display title. JSON
    /// arguments and results are bounded by the node before sending.
    #[derive(Default)]
    pub struct AgentToolCall {
        pub name: Option<String>,
        pub input: Option<String>,
        pub input_truncated: bool,
        pub result: Option<String>,
        pub result_truncated: bool,
    }
}

wire_enum! {
    /// Who an entry is from: a prompt, the agent's text, a tool call, a
    /// permission question and its answer, or what the agent thought and
    /// for how many milliseconds (`None` while it still thinks, or when
    /// that is not known).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
    pub enum AgentEntryKind {
        0 User {}
        1 Agent {}
        2 Tool {}
        3 Question {}
        4 Other {}
        5 Thought { ms: Option<u64> }
    }
}

wire_enum! {
    /// What `SteerAgentSession` does to a session: `Prompt` sends it a
    /// message after the running turn, `Cancel` stops the turn and drops
    /// waiting messages, `Close` ends the session and stops the agent.
    /// `PromptImages` is `Prompt` with images after the text, as
    /// `check_agent_images` allows them; `text` may then be empty. An
    /// agent that takes no images gets the text alone. `Configure` switches the running session
    /// to `model` and thinking `effort` (either may be `None` to keep it),
    /// from its next turn; a node or agent that cannot switch refuses it.
    /// `Steer` is `PromptImages` for the running turn: the node delivers it
    /// once the agent's current tool call finishes rather than after the
    /// whole turn, and like `Prompt` when no turn runs. `DropSteers` takes back the steers
    /// not yet delivered. `SetMode` switches the running session to `mode`, one
    /// `GetAgentSessionModes` lists.
    /// `StopTask` stops one of the commands `GetAgentSessionTasks` lists
    /// without ending the turn.
    /// `Cancel` stops only the main agent's turn; `StopAll` also ends the
    /// session's subagents, background commands and the agents it started.
    /// `SetDevice` keeps the session's device, or forgets it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum AgentSteer {
        0 Prompt { text: String }
        1 Cancel {}
        2 Close {}
        3 PromptImages { text: String, images: Vec<Upload> }
        4 Configure { model: Option<ModelId>, effort: Option<Effort> }
        5 Steer { text: String, images: Vec<Upload> }
        6 DropSteers {}
        7 SetMode { mode: String }
        8 StopTask { task: String }
        9 StopAll {}
        10 SetDevice { device: Option<SessionDevice> }
        11 ConfigureFast { model: Option<ModelId>, effort: Option<Effort>, fast: bool }
    }
}

/// The image types `AgentSteer::PromptImages` may carry.
const AGENT_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

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

wire_structs! {
    /// A coding-agent session on one of the user's machines.
    pub struct MachineSession {
        /// The `Machine::id` it runs on.
        pub machine_id: String,
        pub machine_name: String,
        /// The node's id for it, or the agent's own for an external session.
        pub id: SessionId,
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
        /// Its subagents that still run; 0 for an external session and from
        /// an older node.
        #[serde(default)]
        pub subagents: u32,
        /// The model it runs on, when its node says.
        #[serde(default)]
        pub model: Option<ModelId>,
        /// The home assistant; false from an older node.
        #[serde(default)]
        pub assistant: bool,
        /// The first line of its last reply, cut short, for the lists to
        /// show while it is idle; `None` before the first and from an older
        /// node.
        #[serde(default)]
        pub reply: Option<String>,
        /// The lines changed in its checkout when its last turn ended;
        /// `None` before then, outside a git checkout and from an older
        /// node.
        #[serde(default)]
        pub changes: Option<LineChanges>,
        /// The named account it runs as; `None` for the agent's default
        /// one, an external session and from an older node.
        #[serde(default)]
        pub account: Option<AccountName>,
        /// The custom agent it runs as; `None` for most and from an older
        /// node.
        #[serde(default)]
        pub persona: Option<Persona>,
        /// Where it works now, as its node worked it out; `None` for an
        /// external session, before the node knows and from an older node.
        #[serde(default)]
        pub place: Option<WorkPlace>,
        /// The session on the same machine this one carries on, likely on
        /// another harness; `None` for most and from an older node.
        #[serde(default)]
        pub continues: Option<SessionId>,
    }
}

wire_enum! {
    /// What a session is doing. `Waiting` and `Running` are only for
    /// external sessions: waiting for the user, or running with nothing
    /// more known.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
    #[strum(serialize_all = "lowercase")]
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

wire_structs! {
    /// A permission question a session asks before it acts.
    pub struct AgentQuestion {
        pub id: u32,
        /// What the agent wants to do, such as "Run cargo test".
        pub text: String,
        /// The command, path or other input it would act on, cut short.
        pub detail: Option<String>,
        pub options: Vec<AgentOption>,
    }

    /// One answer to an `AgentQuestion`.
    pub struct AgentOption {
        pub id: String,
        pub name: String,
        /// It lets the agent go ahead, once or always.
        pub allow: bool,
    }
}

/// Starts a coding-agent session on `prompt` in `cwd` (the machine's
/// workspace or home folder when `None`). Without `machine_id`, Auto picks
/// a machine that offers `agent` (any when `None`) and has every capability
/// in `needs` and those the prompt and folder call for. `model` is an id
/// from `ListAgentModels`, the agent's default when `None`; `effort` is one
/// of `node::agent::efforts(agent)`. `checkout` first makes a branch or
/// worktree in `cwd`'s git checkout, named from the prompt. `assistant`
/// starts the home assistant, which only newer nodes run.
#[derive(Debug, Clone, PartialEq)]
pub struct StartAgentSession {
    pub fast: Option<bool>,
    pub machine_id: Option<String>,
    pub agent: Option<String>,
    pub cwd: Option<String>,
    pub prompt: String,
    pub needs: Vec<MachineCapability>,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub checkout: Option<CheckoutPlan>,
    pub assistant: bool,
    /// The agent's named account to sign in as; `None` for its default one.
    pub account: Option<AccountName>,
    /// The session on the same machine the new one carries on.
    pub continues: Option<SessionId>,
}

/// Resumes a session started outside Beep, by the agent's own id: it then
/// runs as a session Beep started, in its folder, on `prompt` when given.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeAgentSession {
    pub machine_id: String,
    pub agent: String,
    pub session: SessionId,
    pub prompt: Option<String>,
    pub model: Option<ModelId>,
}

/// One session with its plan, the files it changed and the last `last`
/// entries of its transcript (at most 200). `before`, when given, is the
/// index of the entry the `last` entries end before, to read further back.
#[derive(Debug, Clone, PartialEq)]
pub struct GetAgentSession {
    pub machine_id: String,
    pub agent: String,
    pub session: SessionId,
    pub last: u32,
    pub before: Option<u64>,
}

/// Sends a session Beep started a message, stops its turn or ends it.
#[derive(Debug, Clone, PartialEq)]
pub struct SteerAgentSession {
    pub machine_id: String,
    pub agent: String,
    pub session: SessionId,
    pub steer: AgentSteer,
}

/// The models the coding agent `agent` can run on `machine_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct ListAgentModels {
    pub machine_id: String,
    pub agent: String,
}

/// The picture of a `[image: image-<key>]` line in a session's log.
#[derive(Debug, Clone, PartialEq)]
pub struct GetAgentImage {
    pub machine_id: String,
    pub session: SessionId,
    pub key: String,
}

/// Up to [`AGENT_FILE_CHUNK`] bytes from `offset` of a media file a coding
/// agent made, such as a video, 3D model, PDF or picture. `path` is
/// absolute or starts `~/`; only files [`media_mime`](crate::media_mime)
/// knows, streamed from disk.
#[derive(Debug, Clone, PartialEq)]
pub struct GetAgentFile {
    pub machine_id: String,
    pub path: String,
    pub offset: u64,
}

impl From<&SessionInfo> for AgentSessionUsage {
    fn from(info: &SessionInfo) -> Self {
        Self {
            context: info.context,
            tokens: info.tokens,
            cost_usd: info.cost_usd,
            credits: info.credits.clone(),
        }
    }
}

/// The modes of a session Beep runs, the one it is in, and the plan it
/// waits on approval of.
#[derive(Debug, Clone, PartialEq)]
pub struct GetAgentSessionModes {
    pub machine_id: String,
    pub agent: String,
    pub session: SessionId,
}

/// The commands a session Beep runs has in the background, running or ended.
#[derive(Debug, Clone, PartialEq)]
pub struct GetAgentSessionTasks {
    pub machine_id: String,
    pub agent: String,
    pub session: SessionId,
}
