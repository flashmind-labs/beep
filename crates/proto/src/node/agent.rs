//! Coding agents on a node, such as Claude Code or Codex, driven over ACP by
//! the daemon.
//!
//! Each agent is its own protocol, `<agent>/1` (`claude/1`, `codex/1`), and
//! all of them speak the messages here. A stream is one agent session: the
//! server opens it with `Start` or `Attach`, the node answers `Attached` with
//! the session as it stands, then the server steers it with `Prompt`,
//! `Answer`, `Cancel` and `Close` (each answered by one `Ack`) while the node
//! sends `State`, `Question` and `TurnDone` as they happen. Sessions live in
//! the daemon, not in the stream: when the stream drops the agent keeps
//! working, and a later `Attach` picks it up again.
//!
//! `agents/1` lists the sessions and reads their transcripts, one request per
//! stream. It also lists the sessions the user started on the machine
//! outside Beep (`External`), such as Claude Code in a terminal, and
//! `Watch` follows them: one request, then `Activity` replies until the
//! server closes the stream. `Resume` takes one of them over once nothing
//! else runs it.

use crate::{AgentCheckoutPlan, AgentEffort, ModelId, Upload};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The version every agent protocol shares.
const VERSION: &str = "1";

/// Lists sessions and reads transcripts.
pub const SESSIONS_PROTOCOL: &str = "agents/1";

/// The protocol for the agent `name`, such as `claude/1`.
pub fn protocol(name: &str) -> String {
    format!("{name}/{VERSION}")
}

/// The agent a protocol name serves, such as `claude` for `claude/1`.
pub fn agent_of(protocol: &str) -> Option<&str> {
    let (name, version) = protocol.split_once('/')?;
    (version == VERSION
        && !matches!(
            name,
            "shell" | "agents" | "desktop" | "files" | "process" | "sync" | "terminal" | "transfer"
        ))
    .then_some(name)
}

/// What the server sends on an agent stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The first message: starts a session in `cwd` (the home folder when
    /// `None`) on `prompt`, on `model` when given (an id from
    /// `SessionsRequest::Models`) and thinking at `effort` when given.
    /// With `checkout`, it first makes the branch or worktree it names in
    /// `cwd`'s git checkout and starts there. A node from before any of
    /// these ignores it; one that makes checkouts says so with
    /// `NodeHello::checkouts`.
    Start {
        cwd: Option<String>,
        prompt: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<Effort>,
        #[serde(default)]
        checkout: Option<CheckoutPlan>,
    },
    /// The first message: follows a session that already runs.
    Attach { session: String },
    /// The first message: takes over a session started outside Beep, by its
    /// agent's own id (see `ExternalSession`), once nothing else runs it.
    /// It then runs here like a started one, under a new id, on `prompt`
    /// when there is one.
    Resume {
        session: String,
        #[serde(default)]
        prompt: Option<String>,
    },
    /// Sends the agent a message. While a turn runs it waits for the turn
    /// to end.
    Prompt { text: String },
    /// `Prompt` with images, sent to the agent after the text. Refused,
    /// not sent without them, when the agent takes no images. A node from
    /// before it answers that it does not know the request.
    PromptImages { text: String, images: Vec<Upload> },
    /// Answers the open question. `reply` is one of its options, by id,
    /// number or name; anything else declines and goes to the agent as the
    /// next prompt.
    Answer { question: u32, reply: String },
    /// Stops the running turn and drops waiting prompts.
    Cancel {},
    /// Ends the session and stops the agent.
    Close {},
    /// Switches the running session to `model` and thinking at `effort`
    /// (`None` keeps it), from its next turn. Refused when the agent cannot
    /// switch. A node from before it answers that it does not know the
    /// request.
    Configure {
        model: Option<ModelId>,
        effort: Option<Effort>,
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
    pub id: String,
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Status {
    /// The agent is starting up.
    #[default]
    Starting,
    /// A turn is running.
    Working,
    /// A turn waits for an answer to `question`.
    Asking,
    /// Waiting for a prompt.
    Idle,
    /// The agent stopped: it failed, exited or was closed.
    Ended,
    /// A status from a newer node.
    #[serde(other)]
    Unknown,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Working => "working",
            Self::Asking => "waiting for an answer",
            Self::Idle => "idle",
            Self::Ended => "ended",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub content: String,
    #[serde(default)]
    pub status: PlanStatus,
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
pub enum OptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    #[default]
    #[serde(other)]
    Other,
}

impl OptionKind {
    /// The kind ACP names `name`, such as `allow_once`.
    pub fn from_acp(name: &str) -> Self {
        match name {
            "allow_once" => Self::AllowOnce,
            "allow_always" => Self::AllowAlways,
            "reject_once" => Self::RejectOnce,
            "reject_always" => Self::RejectAlways,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowAlways => "allow_always",
            Self::RejectOnce => "reject_once",
            Self::RejectAlways => "reject_always",
            Self::Other => "other",
        }
    }

    pub fn rejects(self) -> bool {
        matches!(self, Self::RejectOnce | Self::RejectAlways)
    }
}

/// Why a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    /// The agent answered the prompt with an error.
    Failed,
    #[serde(other)]
    Other,
}

impl StopReason {
    /// The reason ACP names `name`, such as `end_turn`.
    pub fn from_acp(name: &str) -> Self {
        match name {
            "end_turn" => Self::EndTurn,
            "max_tokens" => Self::MaxTokens,
            "max_turn_requests" => Self::MaxTurnRequests,
            "refusal" => Self::Refusal,
            "cancelled" => Self::Cancelled,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndTurn => "finished",
            Self::MaxTokens => "hit the token limit",
            Self::MaxTurnRequests => "hit the request limit",
            Self::Refusal => "refused",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Other => "stopped",
        }
    }
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the server sends on an `agents/1` stream: one request, one reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionsRequest {
    List {},
    /// The last `last` entries of a session's transcript. `session` may
    /// also be an external session's id.
    Transcript {
        session: String,
        last: u32,
    },
    /// Sessions of `agent` started outside Beep, newest first: at most
    /// `limit`, or only `session` when given.
    External {
        agent: String,
        #[serde(default)]
        limit: u32,
        #[serde(default)]
        session: Option<String>,
    },
    /// Follows these external sessions: `Watching` first, then `Activity`
    /// as they finish turns or wait for the user, until the server closes
    /// the stream.
    Watch {
        sessions: Vec<String>,
    },
    /// The git checkouts coding agents worked in, from the sessions Claude
    /// Code, Codex, pi, flsh and OpenCode keep: at most `limit`, newest activity
    /// first. Answered with `Folders`. A node from before it answers
    /// `Error` or closes the stream.
    Folders {
        #[serde(default)]
        limit: u32,
    },
    /// The sessions those agents ran in any of `folders` (or a subfolder or
    /// worktree of one), newest first, at most `limit`. Answered with
    /// `External`, `title` falling back to the first prompt.
    InFolders {
        folders: Vec<String>,
        #[serde(default)]
        limit: u32,
    },
    /// The picture of the `[image: image-<key>]` line in `session`'s
    /// transcript, `key` being its `image_key`. Answered with `Image`, or
    /// `Error` when the node no longer has it. A node from before it
    /// closes the stream.
    Image {
        session: String,
        key: String,
    },
    /// The models the agent `agent` can run on here, for `Start`'s
    /// `model`. Answered with `Models`, or `Error` for an agent the node
    /// does not have. A node from before it closes the stream.
    Models {
        agent: String,
    },
    /// What the node finds of each coding agent harness it knows, answered
    /// with `Harnesses`. `refresh` looks the newest versions up again
    /// instead of using the ones found lately. A node from before it
    /// answers `Error` or closes the stream.
    Harnesses {
        #[serde(default)]
        refresh: bool,
    },
    /// Installs the harness `agent`, or updates it to the newest version,
    /// with the command its `HarnessInfo::install` names, then answers
    /// `Harnesses` as they stand after, or `Error` saying why it failed.
    UpdateHarness {
        agent: String,
    },
    /// Sends each picture a running session's transcript newly shows, as
    /// `SessionImage` replies, until the server closes the stream; the
    /// server keeps them so they show while the node is offline. Pictures
    /// that came while no stream was open wait, a bounded few. A node from
    /// before it closes the stream.
    FollowImages {},
    /// Sends what each finished turn spent, as `TurnUsage` replies, and
    /// how much of each agent's plan is left, as `PlanUsage` replies (all
    /// of them first, then each time one changes), until the server closes
    /// the stream. Turns that ended while no stream was open wait, a
    /// bounded few. On this stream the server may later send
    /// `RefreshPlanUsage`; anything else is its goodbye. A node from before
    /// it closes the stream.
    FollowUsage {},
    /// Sent on a `FollowUsage` stream: reads the plans' usage again now,
    /// rather than at the next check.
    RefreshPlanUsage {},
    /// Up to `AGENT_FILE_CHUNK` bytes from `offset` of the media file
    /// `path`, absolute or starting `~/` for the node's home. Answered with
    /// `File`, `NoFile` when there is no such file, or `Error` saying why it
    /// cannot be read. A node from before it answers `Error` or closes the
    /// stream.
    File {
        path: String,
        offset: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionsReply {
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    Transcript {
        entries: Vec<Entry>,
        /// Entries before these ones.
        earlier: u32,
    },
    Error {
        message: String,
    },
    External {
        sessions: Vec<ExternalSession>,
    },
    /// The answer to `Watch`: the sessions it could not find.
    Watching {
        unknown: Vec<String>,
    },
    Activity {
        activity: Activity,
    },
    Folders {
        folders: Vec<SessionFolder>,
    },
    /// The answer to `Image`.
    Image {
        mime: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// The answer to `Models`.
    Models {
        models: ModelList,
    },
    Harnesses {
        report: super::harness::HarnessReport,
    },
    /// One picture on a `FollowImages` stream: of `session`, shown in its
    /// transcript by the line of its `image_key`.
    SessionImage {
        session: String,
        mime: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// One finished turn on a `FollowUsage` stream.
    TurnUsage {
        usage: super::usage::TurnUsage,
    },
    /// One agent's plan usage on a `FollowUsage` stream.
    PlanUsage {
        usage: super::usage::PlanUsage,
    },
    /// The answer to `File`: the whole file's `size` and its MIME type,
    /// with the bytes from the asked offset, empty at or past the end.
    File {
        size: u64,
        mime: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// The answer to `File` when there is no file at its path.
    NoFile {},
}

/// The models an agent can run on a node, as `SessionsRequest::Models`
/// finds them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelList {
    pub pick: ModelPick,
    /// The model the agent runs when none is picked, when it says.
    pub current: Option<String>,
    /// The models to pick from, for `ModelPick::Listed`.
    pub models: Vec<ModelOption>,
}

/// The longest model id a start may name.
pub const MAX_MODEL: usize = 200;

/// How hard a coding agent thinks. Its name is the value each agent's ACP
/// adapter takes on its config option of category `thought_level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[non_exhaustive]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// The value an agent's `thought_level` option takes.
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

impl From<AgentEffort> for Effort {
    fn from(effort: AgentEffort) -> Self {
        match effort {
            AgentEffort::Off {} => Self::Off,
            AgentEffort::Minimal {} => Self::Minimal,
            AgentEffort::Low {} => Self::Low,
            AgentEffort::Medium {} => Self::Medium,
            AgentEffort::High {} => Self::High,
            AgentEffort::Xhigh {} => Self::Xhigh,
            AgentEffort::Max {} => Self::Max,
        }
    }
}

impl From<Effort> for AgentEffort {
    fn from(effort: Effort) -> Self {
        match effort {
            Effort::Off => Self::Off {},
            Effort::Minimal => Self::Minimal {},
            Effort::Low => Self::Low {},
            Effort::Medium => Self::Medium {},
            Effort::High => Self::High {},
            Effort::Xhigh => Self::Xhigh {},
            Effort::Max => Self::Max {},
        }
    }
}

/// The thinking levels the agent `agent` takes at start; none for an agent
/// whose thinking cannot be set from here. Each mirrors its CLI, whose
/// ACP adapter offers the same levels as a `thought_level` option:
///
/// - `claude`: `claude --effort` (low, medium, high, xhigh, max).
/// - `codex`: `codex -c model_reasoning_effort=` (low, medium, high).
/// - `pi`: `pi --thinking` (off, minimal, low, medium, high, xhigh, max).
///
/// Gemini CLI, flsh and OpenCode have no such flag: flsh keeps it in its
/// own config.
pub fn efforts(agent: &str) -> &'static [Effort] {
    use Effort::*;
    match agent {
        "claude" => &[Low, Medium, High, Xhigh, Max],
        "codex" => &[Low, Medium, High],
        "pi" => &[Off, Minimal, Low, Medium, High, Xhigh, Max],
        _ => &[],
    }
}

/// A new branch a session starts on, made from `base` (a branch, tag or
/// commit; the checkout's `HEAD` when `None`). Its name comes from the
/// prompt, as `prompt_branch` makes it, with `-2`, `-3` added while that
/// is taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CheckoutPlan {
    /// Switches the checkout itself to the branch; refused when it has
    /// uncommitted changes.
    NewBranch { base: Option<String> },
    /// Adds a worktree on the branch in the checkout, where
    /// `worktree_path` puts it, and starts there.
    NewWorktree { base: Option<String> },
    /// `NewWorktree` on a branch named `branch` (with `-2`, `-3` added
    /// while that is taken) rather than from the prompt. A node from before
    /// it does not read the start; one that makes it says so with
    /// `NodeHello::branch_checkouts`.
    NamedWorktree {
        base: Option<String>,
        branch: String,
    },
    /// Starts on the existing branch `branch`: in the worktree that has it
    /// checked out, else in a new one where `worktree_path` puts it, made
    /// after fetching `branch` from `origin`. Like `NamedWorktree`, only
    /// for a node with `NodeHello::branch_checkouts`.
    BranchWorktree { branch: String },
}

impl CheckoutPlan {
    /// The ref the branch is made from, when one is named.
    pub fn base(&self) -> Option<&str> {
        match self {
            Self::NewBranch { base }
            | Self::NewWorktree { base }
            | Self::NamedWorktree { base, .. } => base.as_deref(),
            Self::BranchWorktree { .. } => None,
        }
    }

    /// The branch it names, for the plans that name one.
    pub fn branch(&self) -> Option<&str> {
        match self {
            Self::NamedWorktree { branch, .. } | Self::BranchWorktree { branch } => Some(branch),
            Self::NewBranch { .. } | Self::NewWorktree { .. } => None,
        }
    }
}

/// Whether `branch` may name a branch a plan makes or starts on: a
/// `valid_ref` that is not `HEAD`, has no `~` or `^`, and no part that is
/// empty, starts with `.` or ends in `.lock`. The node checks it again.
pub fn valid_branch(branch: &str) -> bool {
    valid_ref(branch)
        && branch != "HEAD"
        && !branch.contains(['~', '^'])
        && branch
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

impl From<AgentCheckoutPlan> for CheckoutPlan {
    fn from(plan: AgentCheckoutPlan) -> Self {
        match plan {
            AgentCheckoutPlan::NewBranch { base } => Self::NewBranch { base },
            AgentCheckoutPlan::NewWorktree { base } => Self::NewWorktree { base },
            AgentCheckoutPlan::NamedWorktree { base, branch } => {
                Self::NamedWorktree { base, branch }
            }
            AgentCheckoutPlan::BranchWorktree { branch } => Self::BranchWorktree { branch },
        }
    }
}

impl From<CheckoutPlan> for AgentCheckoutPlan {
    fn from(plan: CheckoutPlan) -> Self {
        match plan {
            CheckoutPlan::NewBranch { base } => Self::NewBranch { base },
            CheckoutPlan::NewWorktree { base } => Self::NewWorktree { base },
            CheckoutPlan::NamedWorktree { base, branch } => Self::NamedWorktree { base, branch },
            CheckoutPlan::BranchWorktree { branch } => Self::BranchWorktree { branch },
        }
    }
}

/// The longest base ref a checkout plan may name.
pub const MAX_REF: usize = 200;

/// Whether `base` may name the ref a new branch starts from: short, not an
/// option, and nothing git refuses in a ref anywhere (spaces, control
/// characters, `..`, `:`). `~` and `^` stay, for `HEAD~2`.
pub fn valid_ref(base: &str) -> bool {
    !base.is_empty()
        && base.len() <= MAX_REF
        && !base.starts_with('-')
        && !base.contains("..")
        && !base.contains("@{")
        && !base.chars().any(|c| {
            c.is_control() || c.is_whitespace() || matches!(c, ':' | '?' | '*' | '[' | '\\')
        })
}

/// The longest slug `slug` makes.
pub const MAX_SLUG: usize = 40;

/// A short name for `text`, for a branch or folder: its ASCII letters and
/// digits in lowercase, each run of anything else as one `-`, at most
/// `MAX_SLUG` long, with no `-` at either end. Empty when `text` has no
/// letter or digit. Project tasks name their branches the same way.
pub fn slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    // Only ASCII is pushed, so any byte index is a char boundary.
    slug.truncate(MAX_SLUG);
    slug.trim_matches('-').to_owned()
}

/// The branch a session started on `prompt` gets: `beep/<slug>`, or
/// `beep/agent` for a prompt without a letter or digit.
pub fn prompt_branch(prompt: &str) -> String {
    let slug = slug(prompt);
    let slug = if slug.is_empty() { "agent" } else { &slug };
    format!("beep/{slug}")
}

/// Where `branch`'s worktree goes in its checkout: `.worktrees/` and the
/// name with each `/` as `-`, as the desktop's `worktreeName` and the
/// Branches tile name it.
pub fn worktree_path(branch: &str) -> String {
    format!(".worktrees/{}", branch.replace('/', "-"))
}

/// Whether `model` may name a model to start an agent on: short, and no
/// control characters, since it may go on the agent's command line.
pub fn valid_model(model: &str) -> bool {
    !model.is_empty() && model.len() <= MAX_MODEL && !model.chars().any(char::is_control)
}

/// How a model is picked for an agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ModelPick {
    /// One of `ModelList::models`.
    Listed,
    /// Any name the agent takes; it lists none.
    Named,
    /// None can be picked: the agent runs its own.
    #[default]
    #[serde(other)]
    Fixed,
}

/// One model an agent offers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelOption {
    /// What `Start`'s `model` names it by.
    pub id: String,
    /// What people call it; the id when the agent gives no name.
    pub name: String,
    pub description: Option<String>,
}

/// A git checkout coding agents worked in, as `SessionsRequest::Folders`
/// finds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionFolder {
    /// The checkout's main folder, absolute.
    pub path: String,
    /// Its `origin` remote; empty when it has none.
    pub origin: String,
    /// Sessions per agent, most first.
    pub agents: Vec<AgentSessions>,
    /// When a session there last changed, Unix seconds.
    pub last_activity: u64,
}

/// How many sessions one agent ran in a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSessions {
    /// Such as `claude` or `codex`.
    pub agent: String,
    pub sessions: u32,
}

/// A session the user started on the machine outside Beep, such as Claude
/// Code in a terminal or an IDE.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalSession {
    /// The agent's own id for it, such as Claude Code's session UUID.
    pub id: String,
    pub agent: String,
    pub cwd: String,
    pub title: Option<String>,
    /// Unix seconds.
    pub started_at: u64,
    /// When its transcript last changed, Unix seconds.
    pub last_activity: u64,
    pub presence: Presence,
    /// The process that runs it, when one does.
    pub pid: Option<u32>,
    /// The user's last prompt, cut short.
    pub last_prompt: String,
    /// The agent's last text, cut short.
    pub last_message: String,
    /// The node session it runs as, once Beep took it over.
    pub taken_over: Option<String>,
}

/// Whether anything runs an external session now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Presence {
    /// Its process runs a turn.
    Working,
    /// Its process waits for the user.
    Waiting,
    /// Its process runs; what it does is not known.
    Running,
    /// No process was found, but its transcript changed a moment ago.
    Recent,
    /// Nothing runs it.
    Stopped,
    #[default]
    #[serde(other)]
    Unknown,
}

impl Presence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting for the user",
            Self::Running => "running",
            Self::Recent => "active a moment ago",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }

    /// A process may run it, so taking it over could clash.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Working | Self::Waiting | Self::Running | Self::Recent
        )
    }
}

impl fmt::Display for Presence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Something a followed external session did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub session: String,
    pub kind: ActivityKind,
    /// The agent's last text or the notice it showed, cut short.
    #[serde(default)]
    pub message: String,
    /// It came from a hook rather than from watching the transcript.
    #[serde(default)]
    pub from_hook: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ActivityKind {
    /// A turn ended.
    TurnDone,
    /// It waits for the user: a permission prompt or an idle prompt.
    NeedsInput,
    /// A process started or resumed it.
    Started,
    /// Its process ended.
    Ended,
    #[serde(other)]
    Other,
}

impl ActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TurnDone => "finished a turn",
            Self::NeedsInput => "is waiting for the user",
            Self::Started => "started",
            Self::Ended => "ended",
            Self::Other => "changed",
        }
    }
}

/// One entry of a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EntryKind {
    /// A prompt sent to the agent.
    User,
    /// What the agent said.
    Agent,
    /// A tool call and how it went.
    Tool,
    /// A permission question and its answer.
    Question,
    #[serde(other)]
    Other,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::Question => "question",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_name_their_agent() {
        assert_eq!(protocol("claude"), "claude/1");
        assert_eq!(agent_of("codex/1"), Some("codex"));
        assert_eq!(agent_of("shell/1"), None);
        assert_eq!(agent_of("files/1"), None);
        assert_eq!(agent_of("process/1"), None);
        assert_eq!(agent_of(crate::node::sync::PROTOCOL), None);
        assert_eq!(agent_of(crate::node::terminal::PROTOCOL), None);
        assert_eq!(agent_of(crate::node::transfer::PROTOCOL), None);
        assert_eq!(agent_of(SESSIONS_PROTOCOL), None);
        assert_eq!(agent_of("claude/2"), None);
    }

    #[test]
    fn a_newer_status_still_reads() {
        #[derive(Serialize)]
        struct Newer {
            status: &'static str,
            summary: &'static str,
            mood: u8,
        }
        let body = rmp_serde::to_vec_named(&Newer {
            status: "Compacting",
            summary: "Thinking",
            mood: 3,
        })
        .unwrap();
        let state: State = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(state.status, Status::Unknown);
        assert_eq!(state.summary, "Thinking");
    }

    #[test]
    fn external_requests_and_replies_round_trip() {
        let request = SessionsRequest::External {
            agent: "claude".into(),
            limit: 20,
            session: None,
        };
        let body = rmp_serde::to_vec_named(&request).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            request
        );
        let reply = SessionsReply::Activity {
            activity: Activity {
                session: "0f3c2a9e-1111-4222-8333-944455556666".into(),
                kind: ActivityKind::NeedsInput,
                message: "Claude needs your permission to use Bash".into(),
                from_hook: true,
            },
        };
        let body = rmp_serde::to_vec_named(&reply).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
            reply
        );
    }

    #[test]
    fn an_external_session_from_a_newer_node_still_reads() {
        #[derive(Serialize)]
        struct Newer {
            id: &'static str,
            presence: &'static str,
            branch: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Newer {
            id: "s1",
            presence: "Compacting",
            branch: "main",
        })
        .unwrap();
        let session: ExternalSession = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(session.id, "s1");
        assert_eq!(session.presence, Presence::Unknown);
    }

    #[test]
    fn model_requests_and_replies_round_trip() {
        let request = SessionsRequest::Models {
            agent: "flsh".into(),
        };
        let body = rmp_serde::to_vec_named(&request).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            request
        );
        let reply = SessionsReply::Models {
            models: ModelList {
                pick: ModelPick::Listed,
                current: Some("openai:gpt-test".into()),
                models: vec![ModelOption {
                    id: "openai:gpt-test".into(),
                    name: "GPT Test".into(),
                    description: None,
                }],
            },
        };
        let body = rmp_serde::to_vec_named(&reply).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
            reply
        );
        let start = Request::Start {
            cwd: None,
            prompt: "go".into(),
            model: Some("openai:gpt-test".into()),
            effort: Some(Effort::Xhigh),
            checkout: Some(CheckoutPlan::NewWorktree {
                base: Some("main".into()),
            }),
        };
        let body = rmp_serde::to_vec_named(&start).unwrap();
        assert_eq!(rmp_serde::from_slice::<Request>(&body).unwrap(), start);
    }

    #[test]
    fn a_start_with_a_model_reads_on_an_older_node_and_an_older_start_here() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum Older {
            Start { cwd: Option<String>, prompt: String },
        }
        let newer = Request::Start {
            cwd: Some("/srv/project".into()),
            prompt: "go".into(),
            model: Some("m".into()),
            effort: Some(Effort::High),
            checkout: Some(CheckoutPlan::NewBranch { base: None }),
        };
        let body = rmp_serde::to_vec_named(&newer).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Older>(&body).unwrap(),
            Older::Start {
                cwd: Some("/srv/project".into()),
                prompt: "go".into()
            }
        );
        let body = rmp_serde::to_vec_named(&Older::Start {
            cwd: None,
            prompt: "go".into(),
        })
        .unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Request>(&body).unwrap(),
            Request::Start {
                cwd: None,
                prompt: "go".into(),
                model: None,
                effort: None,
                checkout: None,
            }
        );
    }

    #[test]
    fn checkout_plans_cross_both_wires() {
        let plans = [
            CheckoutPlan::NewBranch { base: None },
            CheckoutPlan::NewWorktree {
                base: Some("origin/main".into()),
            },
        ];
        for plan in plans {
            assert_eq!(
                CheckoutPlan::from(AgentCheckoutPlan::from(plan.clone())),
                plan
            );
            let body = rmp_serde::to_vec_named(&plan).unwrap();
            assert_eq!(rmp_serde::from_slice::<CheckoutPlan>(&body).unwrap(), plan);
        }
    }

    #[test]
    fn a_prompt_names_its_branch_and_worktree() {
        assert_eq!(slug("Fix the Android build!"), "fix-the-android-build");
        assert_eq!(slug("  Ünïcode -- and   spaces  "), "n-code-and-spaces");
        assert_eq!(slug("!!!"), "");
        let long = slug(&format!("{} {}", "a".repeat(39), "b".repeat(20)));
        assert_eq!(long, "a".repeat(39), "cut at 40, then the hyphen goes");
        assert_eq!(
            prompt_branch("Add a dark mode\nto settings"),
            "beep/add-a-dark-mode-to-settings"
        );
        assert_eq!(prompt_branch("?"), "beep/agent");
        assert_eq!(worktree_path("beep/fix-it-2"), ".worktrees/beep-fix-it-2");
    }

    #[test]
    fn base_refs_are_plain_names() {
        for good in ["main", "origin/main", "v1.2.0", "HEAD~2", "a1b2c3d"] {
            assert!(valid_ref(good), "{good}");
        }
        let long = "x".repeat(MAX_REF + 1);
        for bad in [
            "",
            "-x",
            "--upload-pack=x",
            "a..b",
            "a b",
            "a:b",
            "a\nb",
            "@{-1}",
            &long,
        ] {
            assert!(!valid_ref(bad), "{bad:?}");
        }
    }

    #[test]
    fn each_agent_takes_the_thinking_levels_its_cli_does() {
        use Effort::*;
        assert_eq!(efforts("claude"), [Low, Medium, High, Xhigh, Max]);
        assert_eq!(efforts("codex"), [Low, Medium, High]);
        assert_eq!(efforts("pi"), [Off, Minimal, Low, Medium, High, Xhigh, Max]);
        for agent in ["gemini", "flsh", "opencode", "other"] {
            assert!(efforts(agent).is_empty(), "{agent}");
        }
        let names: Vec<&str> = efforts("pi").iter().map(|e| e.as_str()).collect();
        assert_eq!(
            names,
            ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
        );
        let body = rmp_serde::to_vec_named(&Xhigh).unwrap();
        assert_eq!(rmp_serde::from_slice::<String>(&body).unwrap(), "xhigh");
        for effort in efforts("pi") {
            assert_eq!(Effort::from(AgentEffort::from(*effort)), *effort);
        }
    }

    #[test]
    fn model_ids_are_short_and_plain() {
        assert!(valid_model("openrouter:anthropic/claude-sonnet-4"));
        assert!(valid_model("gpt-5.1-codex[high]"));
        assert!(!valid_model(""));
        assert!(!valid_model("a\nb"));
        assert!(!valid_model(&"m".repeat(MAX_MODEL + 1)));
    }

    #[test]
    fn a_newer_model_pick_reads_as_fixed() {
        #[derive(Serialize)]
        struct Newer {
            pick: &'static str,
            current: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Newer {
            pick: "Searched",
            current: "m",
        })
        .unwrap();
        let list: ModelList = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(list.pick, ModelPick::Fixed);
        assert_eq!(list.current.as_deref(), Some("m"));
    }

    #[test]
    fn events_round_trip() {
        let event = Event::Question {
            question: Question {
                id: 1,
                text: "Run cargo test".into(),
                detail: None,
                options: vec![AnswerOption {
                    id: "allow".into(),
                    name: "Allow".into(),
                    kind: OptionKind::AllowOnce,
                }],
            },
        };
        let body = rmp_serde::to_vec_named(&event).unwrap();
        assert_eq!(rmp_serde::from_slice::<Event>(&body).unwrap(), event);
    }
}
