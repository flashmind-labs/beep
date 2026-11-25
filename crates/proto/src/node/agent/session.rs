//! One agent session's stream: what the caller asks and what the node sends.

use super::{CheckoutPlan, Effort};
use crate::{AccountName, SessionId};
use crate::{
    AgentContext, AgentToolCall, ModelId, ProviderCredits, Upload, node::device::DeviceId,
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use strum::ParseError;

/// What the caller sends on an agent stream.
/// A node older than a request answers that it does not know it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The first message: starts a session in `cwd` (the home folder when
    /// `None`) on `prompt`, on `model` when given (an id from
    /// `SessionsRequest::Models`) and thinking at `effort` when given.
    /// With `checkout`, it first makes the branch or worktree it names in
    /// `cwd`'s git checkout and starts there. `images` go with the first
    /// prompt, as `PromptImages` sends them. A node from before any of
    /// these ignores it.
    Start {
        cwd: Option<String>,
        prompt: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<Effort>,
        #[serde(default)]
        checkout: Option<CheckoutPlan>,
        #[serde(default)]
        images: Vec<Upload>,
        /// The home assistant: the session the user talks to about their
        /// machines. A node from before it runs the session as any other.
        #[serde(default)]
        assistant: bool,
        #[serde(default)]
        fast: Option<bool>,
    },
    /// The first message: follows a session that already runs.
    Attach { session: SessionId },
    /// The first message: takes over a session started outside Beep, by its
    /// agent's own id (see `ExternalSession`), once nothing else runs it.
    /// It then runs here like a started one, under a new id, on `prompt`
    /// when there is one.
    Resume {
        session: SessionId,
        #[serde(default)]
        prompt: Option<String>,
        /// The model to load the session with, when chosen in the app.
        #[serde(default)]
        model: Option<ModelId>,
    },
    /// Sends the agent a message. While a turn runs it waits for the turn
    /// to end.
    Prompt { text: String },
    /// `Prompt` with images, sent to the agent after the text. An agent that
    /// takes no images gets the text alone.
    PromptImages { text: String, images: Vec<Upload> },
    /// Answers the open question. `reply` is one of its options, by id,
    /// number or name; anything else declines and goes to the agent as the
    /// next prompt.
    Answer { question: u32, reply: String },
    /// Stops the main agent's running turn and drops waiting prompts.
    /// `Steer`s not yet sent go as the next prompt once the turn stops;
    /// `DropSteers` first takes them back. Subagents, background commands
    /// and the agents the session started keep running; `StopAll` ends them.
    Cancel {},
    /// Ends the session and stops the agent.
    Close {},
    /// Switches the running session to `model` and thinking at `effort` (`None`
    /// keeps it), from its next turn. Refused when the agent cannot switch.
    Configure {
        model: Option<ModelId>,
        effort: Option<Effort>,
        #[serde(default)]
        fast: Option<bool>,
    },
    /// A message for the running turn: sent once the agent's current tool call
    /// finishes, by stopping the turn there and prompting again, and ahead of
    /// any `Prompt` waiting. With no turn running it goes as a `Prompt`.
    Steer { text: String, images: Vec<Upload> },
    /// Takes back the `Steer`s not yet sent to the agent.
    DropSteers {},
    /// Switches the running session to `mode`, one of `SessionInfo::modes`.
    SetMode { mode: String },
    /// Stops the background task `task` (one of `State::tasks`) without
    /// ending the turn. Refused when it is not running or cannot be stopped.
    StopTask { task: String },
    /// `Cancel`, and also ends the session's background commands, its
    /// subagents and the agents it started. When the agent cannot stop its
    /// subagents one by one, the node stops the agent: the session ends and
    /// the next message resumes it. A node older than this refuses it.
    StopAll {},
    /// Keeps `device` as the session's `SessionInfo::device`, or forgets it.
    /// A node older than this refuses it.
    SetDevice { device: Option<SessionDevice> },
    /// The first message: `start`, a `Start`, signed in as the user's
    /// account `account` of the agent. A node from before named accounts
    /// refuses it, so the session never runs on another account.
    StartAs {
        account: AccountName,
        start: Box<Request>,
    },
}

/// What the node sends on an agent stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    /// The answer to `Start` or `Attach`.
    Attached { session: Box<SessionInfo> },
    /// The answer to `Start` or `Attach` when there is no session.
    Refused { reason: String },
    /// One per request after the first, in order. `error` says why it did
    /// nothing.
    Ack {
        #[serde(default)]
        error: Option<String>,
    },
    /// The session's state changed.
    State { state: State },
    /// The agent asks before it acts.
    Question { question: Question },
    /// A turn ended. `turn` counts the turns so far.
    TurnDone {
        turn: u32,
        stop_reason: StopReason,
        last_message: String,
    },
    /// The session ended; the stream ends after this.
    Ended { reason: String },
}

/// A session as the node keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionInfo {
    /// Short and unique on the node, such as `a3f9c1`.
    pub id: SessionId,
    /// The agent, such as `claude`.
    pub agent: String,
    pub cwd: String,
    /// The title the agent gave the session, when it gives one.
    pub title: Option<String>,
    /// Unix seconds.
    pub started_at: u64,
    pub state: State,
    pub question: Option<Question>,
    /// The agent's last message, cut short.
    pub last_message: String,
    pub turns: u32,
    /// How full the context window is, as the agent last said.
    pub context: Option<AgentContext>,
    /// Every token the finished turns used.
    pub tokens: u64,
    /// The known cost so far at API prices, in US dollars, when the agent reports it.
    pub cost_usd: Option<f64>,
    /// The modes it can be switched between, in the order a switch goes
    /// through them; empty for an agent that offers none.
    pub modes: Vec<ModeOption>,
    /// The id of the mode it is in, once the agent said.
    pub mode: Option<String>,
    /// The plan `question` asks to approve, in Markdown.
    pub plan_ready: Option<String>,
    /// Where it works now, as the node worked it out; `None` before the
    /// node knows or when the folder is in no git checkout.
    pub place: Option<WorkPlace>,
    /// The session that started this one with `start_agent`, when one did.
    pub parent: Option<ParentLink>,
    /// When a prompt last went in or a turn last ended, Unix seconds; 0
    /// before the first. New fields go after it: readers skip what they do
    /// not know.
    pub last_activity: u64,
    /// A temporary reviewer of its parent's changes. Only `ListAll` lists
    /// reviewers.
    pub reviewer: bool,
    /// Provider balances as this session last reported them.
    #[serde(default)]
    pub credits: Vec<ProviderCredits>,
    /// The model this session reports, when known.
    #[serde(default)]
    pub model: Option<ModelId>,
    /// The emulator or simulator the session last used, which its Device
    /// tab shows while it runs.
    #[serde(default)]
    pub device: Option<SessionDevice>,
    /// The linked worktrees the session created, as git names them: one
    /// Beep made when it started, and the ones its commands added. Some
    /// may be gone since.
    #[serde(default)]
    pub worktrees: Vec<String>,
    /// The home assistant (see `Request::Start`).
    #[serde(default)]
    pub assistant: bool,
    /// The lines changed in its checkout when its last turn ended; `None`
    /// before then and outside a git checkout.
    #[serde(default)]
    pub changes: Option<LineChanges>,
    /// The named account it runs as, whose plan its limits are; `None` for
    /// the agent's default one.
    #[serde(default)]
    pub account: Option<AccountName>,
    /// The custom agent it runs as, when its parent started it as one.
    #[serde(default)]
    pub persona: Option<Persona>,
}

/// The longest reply a list row shows, in characters.
const REPLY_CHARS: usize = 160;

impl SessionInfo {
    /// The first line of the agent's last message that has words, cut to what
    /// a list row shows.
    pub fn reply(&self) -> Option<String> {
        let line = self
            .last_message
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())?;
        Some(line.chars().take(REPLY_CHARS).collect())
    }

    /// Only what a list of sessions shows. The per-turn fields (token counts,
    /// context, plan, touched files, tasks, the rest of the last message)
    /// change many times a second while a turn runs and belong to the
    /// session's own `agent` topic and its stream, so a list that carried
    /// them would be sent again on every one.
    pub fn row(&self) -> Self {
        Self {
            id: self.id.clone(),
            agent: self.agent.clone(),
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            started_at: self.started_at,
            state: State {
                status: self.state.status,
                summary: self.state.summary.clone(),
                subagents: self.state.subagents,
                ..State::default()
            },
            question: self.question.clone(),
            last_message: self.reply().unwrap_or_default(),
            model: self.model.clone(),
            assistant: self.assistant,
            changes: self.changes,
            account: self.account.clone(),
            persona: self.persona.clone(),
            place: self.place.clone(),
            parent: self.parent.clone(),
            last_activity: self.last_activity,
            reviewer: self.reviewer,
            ..Self::default()
        }
    }
}

/// A custom agent a session runs as (`CustomAgent`): its name and the
/// color its file gives, such as `blue` or `#3b82f6`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Persona {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

impl From<&crate::CustomAgent> for Persona {
    fn from(agent: &crate::CustomAgent) -> Self {
        Self {
            name: agent.name.clone(),
            color: agent.color.clone(),
        }
    }
}

/// Lines added and removed in a checkout, as its diff counts them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineChanges {
    pub additions: u32,
    pub deletions: u32,
}

/// An emulator or simulator a session uses: the machine it runs on and its id there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDevice {
    pub machine: EndpointId,
    pub device: DeviceId,
}

/// Who started a session and how deep the chain goes, as the node that
/// started it recorded and as it passes it across machines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentLink {
    /// The machine the parent runs on; `None` for a node out of a mesh.
    #[serde(default)]
    pub machine: Option<EndpointId>,
    /// The parent's id: the agent's own when it has one.
    pub session: SessionId,
    /// The id the parent's node listed it under when this was made, which
    /// the agent's own id replaces once the node restarts.
    #[serde(default)]
    pub key: SessionId,
    /// The session at the top of the chain, and its machine.
    #[serde(default)]
    pub root_machine: Option<EndpointId>,
    pub root: SessionId,
    /// How many starts deep this session is: 1 for a child of a root.
    pub depth: u8,
    /// Not read: the node that starts a child counts the root's agents on
    /// every machine itself. Kept so the fields after it keep their place.
    #[serde(default)]
    pub others: u8,
    /// The parent's agent, such as `claude`.
    #[serde(default)]
    pub agent: String,
    /// The mode the parent was held to when it started this one, when it
    /// was not in full access: a child may not run above it.
    #[serde(default)]
    pub mode: Option<String>,
    /// Started in the background: shown only under its parent, and deleted
    /// once it closes, which it does when its parent closes.
    #[serde(default)]
    pub background: bool,
}

/// The git checkout a session works in, as its node works it out from what
/// the agent records: its latest folder, else its last tool call's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkPlace {
    /// The top of the checkout holding the folder.
    pub folder: String,
    /// The main folder of the repository: the checkout `folder` was added
    /// to when it is a linked worktree, else `folder`.
    pub main: String,
    /// `folder` is a linked worktree, not the repository's main checkout.
    pub linked: bool,
    /// The branch checked out; `None` when the head is detached.
    pub branch: Option<String>,
    /// The `origin` remote's URL; empty when there is none.
    pub origin: String,
}

/// A session mode an agent offers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeOption {
    pub id: String,
    pub name: String,
}

/// What a session is doing, small enough to send on every change.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub status: Status,
    /// What the agent is doing now, such as "Edit src/main.rs".
    pub summary: String,
    pub plan: Vec<PlanEntry>,
    /// Files the agent changed, newest last.
    pub files_touched: Vec<String>,
    /// Prompts waiting for the running turn to end.
    pub queued: u32,
    /// Why the agent failed or the last turn went wrong.
    pub error: Option<String>,
    /// The commands the agent runs in the background, oldest first. Only
    /// an agent that reports them has any; a task stays after it ended.
    pub tasks: Vec<Task>,
    /// Text of messages waiting to be delivered, in delivery order.
    #[serde(default)]
    pub queued_messages: Vec<String>,
    /// Subagents the session started that still run. An `Idle` session
    /// with any runs until they finish.
    #[serde(default)]
    pub subagents: u32,
    /// Why the last turn ended; none before the first.
    #[serde(default)]
    pub last_stop: Option<StopReason>,
}

impl State {
    /// A turn runs, or the session waits on subagents that still run.
    pub fn runs(&self) -> bool {
        match self.status {
            Status::Idle => self.subagents > 0,
            Status::Ended => false,
            _ => true,
        }
    }
}

/// A command the agent left running in the background.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    /// The agent's own id for it.
    pub id: String,
    pub name: String,
    pub description: String,
    pub state: TaskState,
    /// The agent can stop it.
    pub can_stop: bool,
    /// The file its output goes to, on the node.
    pub output_file: Option<String>,
    /// The tool call that started it, by the agent's id for the call.
    pub tool_call: Option<String>,
    /// How it ended, when the agent says.
    pub summary: Option<String>,
    /// The transcript entry of the tool call that started it, counted from
    /// the start of the session, once the call is known.
    pub entry: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[derive(strum::AsRefStr, strum::IntoStaticStr, strum::EnumString)]
pub enum TaskState {
    #[default]
    #[strum(serialize = "running")]
    Running,
    #[strum(serialize = "paused")]
    Paused,
    #[strum(serialize = "completed")]
    Completed,
    #[strum(serialize = "failed")]
    Failed,
    #[strum(serialize = "stopped")]
    Stopped,
    /// A state from a newer node.
    #[serde(other)]
    #[strum(serialize = "unknown")]
    Unknown,
}

impl TaskState {
    pub fn ended(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[derive(strum::AsRefStr, strum::IntoStaticStr, strum::Display)]
pub enum Status {
    /// The agent is starting up.
    #[default]
    #[strum(serialize = "starting")]
    Starting,
    /// A turn is running.
    #[strum(serialize = "working")]
    Working,
    /// A turn waits for an answer to `question`.
    #[strum(serialize = "waiting for an answer")]
    Asking,
    /// Waiting for a prompt.
    #[strum(serialize = "idle")]
    Idle,
    /// The agent stopped: it failed, exited or was closed.
    #[strum(serialize = "ended")]
    Ended,
    /// A status from a newer node.
    #[serde(other)]
    #[strum(serialize = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub content: String,
    #[serde(default)]
    pub status: PlanStatus,
    /// Unix seconds when the step first ran, and when it stopped running;
    /// absent from an older node and for a step of loaded history.
    #[serde(default)]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub ended_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlanStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    #[serde(other)]
    Unknown,
}

/// The longest question `detail` the node sends: it cuts longer inputs
/// without a marker, so a question this long may be only the start of a
/// command.
pub const MAX_QUESTION_LINE: usize = 300;

/// A permission question from the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    /// Counts up within the session.
    pub id: u32,
    /// What the agent wants to do, such as "Run cargo test".
    pub text: String,
    /// The command, path or other input it would act on, cut short.
    #[serde(default)]
    pub detail: Option<String>,
    pub options: Vec<AnswerOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: OptionKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[derive(strum::AsRefStr, strum::IntoStaticStr, strum::EnumString)]
pub enum OptionKind {
    #[strum(serialize = "allow_once")]
    AllowOnce,
    #[strum(serialize = "allow_always")]
    AllowAlways,
    #[strum(serialize = "reject_once")]
    RejectOnce,
    #[strum(serialize = "reject_always")]
    RejectAlways,
    #[default]
    #[serde(other)]
    #[strum(serialize = "other")]
    Other,
}

impl OptionKind {
    pub fn rejects(self) -> bool {
        matches!(self, Self::RejectOnce | Self::RejectAlways)
    }
}

/// Why a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[derive(strum::AsRefStr, strum::IntoStaticStr, strum::Display)]
pub enum StopReason {
    #[strum(serialize = "finished")]
    EndTurn,
    #[strum(serialize = "hit the token limit")]
    MaxTokens,
    #[strum(serialize = "hit the request limit")]
    MaxTurnRequests,
    #[strum(serialize = "refused")]
    Refusal,
    #[strum(serialize = "cancelled")]
    Cancelled,
    /// The agent answered the prompt with an error.
    #[strum(serialize = "failed")]
    Failed,
    #[serde(other)]
    #[strum(serialize = "stopped")]
    Other,
}

/// One entry of a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
    /// How long a thought took, in milliseconds, once it ended. Only on
    /// `EntryKind::Thought` entries.
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub tool: Option<AgentToolCall>,
    /// Unix seconds when this entry began; absent for undated history.
    #[serde(default)]
    pub sent_at: Option<u64>,
}

impl Entry {
    pub fn new(kind: EntryKind, text: String) -> Self {
        Self {
            kind,
            text,
            ms: None,
            tool: None,
            sent_at: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[derive(strum::AsRefStr, strum::IntoStaticStr)]
pub enum EntryKind {
    /// A prompt sent to the agent.
    #[strum(serialize = "user")]
    User,
    /// What the agent said.
    #[strum(serialize = "agent")]
    Agent,
    /// A tool call and how it went.
    #[strum(serialize = "tool")]
    Tool,
    /// A permission question and its answer.
    #[strum(serialize = "question")]
    Question,
    /// What the agent thought, as its `agent_thought_chunk`s said it.
    #[strum(serialize = "thought")]
    Thought,
    /// The conversation was compacted.
    #[strum(serialize = "compacted")]
    Compacted,
    #[serde(other)]
    #[strum(serialize = "other")]
    Other,
}

impl FromStr for StopReason {
    type Err = ParseError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "end_turn" => Ok(Self::EndTurn),
            "max_tokens" => Ok(Self::MaxTokens),
            "max_turn_requests" => Ok(Self::MaxTurnRequests),
            "refusal" => Ok(Self::Refusal),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseError::VariantNotFound),
        }
    }
}
