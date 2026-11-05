//! A session as the daemon keeps it: the compact state the server follows,
//! built from the agent's `session/update` notifications, and the
//! transcript, which stays here.

use super::{acp::Prompt, images::Images, usage::UsageHub};
use agent_client_protocol::schema::v1::{CompactionId, ToolCallId, ToolKind};
use beep_proto::node::{
    agent::{AgentCommand, Entry, SessionInfo, StopReason},
    usage::ModelTokens,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::{Instant, SystemTime},
};

mod content;
mod tasks;
mod tools;
mod transcript;
mod updates;

pub(crate) use content::cut;
use tools::ToolCall;

/// The most transcript entries kept; older ones are dropped.
const MAX_ENTRIES: usize = 5000;
/// The longest entry kept.
const MAX_ENTRY: usize = 16 * 1024;
/// The longest last message sent to the server.
const MAX_LAST_MESSAGE: usize = 4 * 1024;
/// The most files and plan entries reported.
const MAX_FILES: usize = 50;
const MAX_PLAN: usize = 30;
/// The most commands kept of those the agent advertises.
const MAX_COMMANDS: usize = 200;
/// The longest command description kept; skills describe themselves at
/// length.
const MAX_DESCRIPTION: usize = 240;
/// The most compactions remembered, so an update of one is shown once.
const MAX_COMPACTIONS: usize = 100;
/// The longest summary or question detail.
const MAX_LINE: usize = beep_proto::node::agent::MAX_QUESTION_LINE;
/// Claude Code's tool that publishes a page or document as an artifact.
const ARTIFACT_TOOL: &str = "Artifact";

/// The `_meta` key claude-agent-acp forwards Claude Code's rate limit
/// events under, in a `usage_update`.
const CLAUDE_RATE_LIMIT: &str = "_claude/rateLimit";

/// The most background tasks kept.
const MAX_TASKS: usize = 20;

/// The most of a thought kept to find its line.
const MAX_THOUGHT: usize = 8 * 1024;

/// The most of a subagent's steps its entry names; it counts them all.
const MAX_STEPS: usize = 60;

/// ACP tool kinds that change files.
const EDIT_KINDS: &[ToolKind] = &[ToolKind::Edit, ToolKind::Delete, ToolKind::Move];

#[derive(Debug, Default)]
pub(crate) struct Live {
    pub(crate) info: SessionInfo,
    /// Why the last turn ended.
    pub(crate) last_stop: Option<StopReason>,
    /// The agent's own id for the session, once it gave one: for Claude
    /// Code, the transcript's name.
    pub(crate) agent_session: Option<String>,
    /// The prompts still waiting when the node stopped, to send once it
    /// resumes the session.
    pub(super) held: Vec<Prompt>,
    /// The slash commands and skills the agent advertises, such as
    /// `compact`.
    pub(crate) commands: Vec<AgentCommand>,
    /// The agent compacted the conversation since the session last asked.
    compacted: bool,
    /// The compactions already shown in the transcript.
    compactions: HashSet<CompactionId>,
    transcript: VecDeque<Entry>,
    /// Entries dropped from the front of `transcript`.
    dropped: u64,
    /// The tool calls seen, by ACP id.
    tools: HashMap<ToolCallId, ToolCall>,
    /// The call that started each Codex subagent, by its thread.
    codex_threads: HashMap<String, ToolCallId>,
    /// The last agent message is still arriving in chunks.
    in_message: bool,
    /// The first replayed prompt is still arriving in chunks and names a
    /// session the agent has not named.
    naming: bool,
    /// Where the pictures its transcript shows are kept, shared by the
    /// node's sessions.
    images: Arc<Images>,
    /// The entry the running turn's prompt is, counted from the start.
    turn_start: u64,
    /// Image files the agent linked to this turn, as it named them.
    turn_links: Vec<String>,
    /// When the running turn started, by the clock files are stamped with.
    turn_began: Option<SystemTime>,
    /// Where finished turns' spending and plan limits go, shared by the
    /// node's sessions.
    usage: Arc<UsageHub>,
    /// The session's cost so far as the agent last said, in US dollars:
    /// the agent reports a running total.
    cost_seen: f64,
    /// What the session cost since the last turn was counted.
    unbilled: Option<f64>,
    /// The image lines of the pictures tool calls showed this turn, each
    /// also shown as an agent image: at most `image_files::MAX_PER_TURN`.
    turn_images: Vec<String>,
    /// The thought arriving now, for the line the summary shows while the
    /// agent thinks.
    thought: String,
    /// The last update was part of `thought`.
    in_thought: bool,
    /// The thought arriving now, as its transcript entry once it has text,
    /// and when it started; `None` for one replayed by a loaded session,
    /// whose time is not known.
    open_thought: Option<OpenThought>,
    /// When the agent last did something other than think: the prompt,
    /// its text, a tool call. Its thinking started then, as agents send
    /// a thought's text late, often all at once as it ends.
    active_at: Option<Instant>,
    /// The folder the last tool call that named one worked in.
    folder: Option<PathBuf>,
    /// A folder whose checkout the session has not looked up yet.
    pub(crate) place_wanted: Option<PathBuf>,
}

/// A thought still arriving.
#[derive(Debug)]
struct OpenThought {
    entry: Option<u64>,
    started: Option<Instant>,
}

/// The tokens one turn used, as its prompt response says.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TurnTokens {
    pub(crate) total: u64,
    pub(crate) models: Vec<ModelTokens>,
}

/// The image files a finished turn named, for the session to read and
/// show with `show_files`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TurnFiles {
    /// The session's folder; only files inside it are read.
    pub(crate) cwd: PathBuf,
    /// Each joined to `cwd`, at most `image_files::MAX_PER_TURN`.
    pub(crate) paths: Vec<PathBuf>,
    /// Files only shell commands or their output named: read when written
    /// since `since`, so a listing does not show old pictures. Within the
    /// same `MAX_PER_TURN` as `paths`.
    pub(crate) made: Vec<PathBuf>,
    pub(crate) since: SystemTime,
    /// The turn's last agent message, which shows them; a new entry when
    /// there is none.
    pub(crate) entry: Option<u64>,
}

#[cfg(test)]
mod tests;
