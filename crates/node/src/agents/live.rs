//! A session as the daemon keeps it: the compact state the server follows,
//! built from the agent's `session/update` notifications, and the
//! transcript, which stays here.

use super::{
    acp::wire_name,
    compaction::{COMPACTED_ENTRY, Usage},
    image_files,
    images::{Images, Picture, data_url},
    subagent::{Activity, Run, Subagent},
    tool_diff::diff_text,
    unix_now,
    usage::{UsageHub, claude_rate_limit},
};
use crate::mcp::calls::Tool as BeepTool;
use agent_client_protocol::schema::{
    MaybeUndefined,
    v1::{
        CompactionId, CompactionStatus, CompactionUpdate, ContentBlock, EmbeddedResourceResource,
        ImageContent, PlanEntryStatus, RequestPermissionRequest, SessionUpdate,
        ToolCall as AcpToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
        ToolCallUpdate, ToolKind,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{
    AgentSlug,
    node::{
        agent::{
            AnswerOption, Entry, EntryKind, OptionKind, PlanEntry, PlanStatus, Question,
            SessionInfo, Status, StopReason,
        },
        usage::{ModelTokens, TurnUsage},
    },
};
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

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
/// The most compactions remembered, so an update of one is shown once.
const MAX_COMPACTIONS: usize = 100;
/// The longest summary or question detail.
const MAX_LINE: usize = beep_proto::node::agent::MAX_QUESTION_LINE;
/// Claude Code's tool that publishes a page or document as an artifact.
const ARTIFACT_TOOL: &str = "Artifact";

/// The `_meta` key claude-agent-acp forwards Claude Code's rate limit
/// events under, in a `usage_update`.
const CLAUDE_RATE_LIMIT: &str = "_claude/rateLimit";

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
    /// How full the context window is, as the agent last said. Kept on
    /// the node: the server is not told.
    pub(crate) context: Option<Usage>,
    /// The names of the slash commands the agent advertises, such as
    /// `compact`.
    pub(crate) commands: Vec<String>,
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
    /// Where the pictures its transcript shows are kept, shared by the
    /// node's sessions.
    images: Arc<Images>,
    /// The entry the running turn's prompt is, counted from the start.
    turn_start: u64,
    /// Image files the agent linked to this turn, as it named them.
    turn_links: Vec<String>,
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
    /// The turn's last agent message, which shows them; a new entry when
    /// there is none.
    pub(crate) entry: Option<u64>,
}

#[derive(Debug)]
struct ToolCall {
    /// Its entry, counted from the start of the session.
    entry: u64,
    title: String,
    kind: ToolKind,
    status: ToolCallStatus,
    paths: Vec<String>,
    /// What it showed, already cut to an entry's length.
    output: String,
    /// The image lines of the pictures it showed.
    pictures: Vec<String>,
    /// The subagent's tool call this one ran in, whose entry shows it.
    parent: Option<ToolCallId>,
    /// The calls a subagent this tool started ran, in order.
    steps: Vec<ToolCallId>,
    /// The subagent it started, when the call says it starts one.
    subagent: Option<Subagent>,
    /// A call to the node's own `beep` server, titled from its arguments.
    beep: Option<BeepCall>,
}

/// A `beep` server call and its arguments as far as they have come.
#[derive(Debug)]
struct BeepCall {
    tool: BeepTool,
    input: Option<Value>,
}

/// A Codex subagent among a session's entries, whose steps are read from
/// its rollout.
#[derive(Debug, Clone)]
pub(crate) struct CodexChild {
    /// The call that started it.
    pub(crate) call: ToolCallId,
    pub(crate) threads: Vec<String>,
}

impl Live {
    pub(crate) fn new(info: SessionInfo, images: Arc<Images>, usage: Arc<UsageHub>) -> Self {
        Self {
            info,
            images,
            usage,
            ..Self::default()
        }
    }

    /// Counts what the session spent since the last count, with `tokens`
    /// the turn used, as a finished turn. Nothing when it spent nothing.
    pub(crate) fn spent(&mut self, tokens: TurnTokens) {
        let cost_usd = self.unbilled.take();
        if tokens.total == 0 && tokens.models.is_empty() && cost_usd.is_none() {
            return;
        }
        self.usage.record(TurnUsage {
            session: self.info.id.clone(),
            agent: AgentSlug::new(self.info.agent.as_str()),
            cwd: self.info.cwd.clone(),
            at: unix_now(),
            cost_usd,
            tokens: tokens.total,
            models: tokens.models,
            agent_session: self.agent_session.clone(),
        });
    }

    /// Takes a `usage_update`'s running cost and the rate limits it carries.
    fn usage_update(
        &mut self,
        cost: Option<(f64, &str)>,
        meta: Option<&serde_json::Map<String, Value>>,
    ) {
        if let Some((amount, "USD")) = cost
            && amount.is_finite()
            && amount >= 0.0
        {
            // A total lower than before starts again, as after a restart.
            let delta = if amount >= self.cost_seen {
                amount - self.cost_seen
            } else {
                amount
            };
            self.cost_seen = amount;
            if delta > 0.0 {
                *self.unbilled.get_or_insert(0.0) += delta;
            }
        }
        if let Some(info) = meta.and_then(|meta| meta.get(CLAUDE_RATE_LIMIT)) {
            self.usage
                .update_windows(&AgentSlug::CLAUDE, claude_rate_limit(info), unix_now());
        }
    }

    /// Keeps a picture the transcript shows, for the server to ask for,
    /// and has it sent to the server to keep.
    pub(crate) fn keep(&self, picture: Picture) {
        self.images.keep_and_send(&self.info.id, picture);
    }

    /// The kept picture `key` of this session.
    #[cfg(test)]
    pub(crate) fn picture(&self, key: &str) -> Option<Arc<Picture>> {
        self.images.get(&self.info.id, key)
    }

    pub(crate) fn status(&self) -> Status {
        self.info.state.status
    }

    /// The Codex subagents shown at entry `from` or later.
    pub(crate) fn codex_children(&self, from: u64) -> Vec<CodexChild> {
        self.tools
            .iter()
            .filter(|(_, tool)| tool.entry >= from && tool.parent.is_none())
            .filter_map(|(id, tool)| {
                let subagent = tool.subagent.as_ref().filter(|sub| sub.codex)?;
                (!subagent.threads.is_empty()).then(|| CodexChild {
                    call: id.clone(),
                    threads: subagent.threads.clone(),
                })
            })
            .collect()
    }

    /// The entry of call `id` with its subagent's `run` read from disk,
    /// and where that entry is.
    pub(crate) fn with_run(&self, id: &ToolCallId, run: &Run) -> Option<(u64, String)> {
        let tool = self.tools.get(id)?;
        Some((tool.entry, tool.entry_with(&self.steps_of(tool), Some(run))))
    }

    /// The last `last` entries and how many came before them.
    pub(crate) fn transcript(&self, last: usize) -> (Vec<Entry>, u64) {
        let skip = self.transcript.len().saturating_sub(last);
        let entries = self.transcript.iter().skip(skip).cloned().collect();
        (entries, self.dropped + skip as u64)
    }

    pub(crate) fn push(&mut self, kind: EntryKind, text: &str) {
        self.end_thought(Instant::now());
        self.in_message = false;
        self.push_entry(kind, cut(text, MAX_ENTRY));
    }

    fn push_entry(&mut self, kind: EntryKind, text: String) -> u64 {
        if self.transcript.len() >= MAX_ENTRIES {
            self.transcript.pop_front();
            self.dropped += 1;
        }
        self.transcript.push_back(Entry::new(kind, text));
        self.dropped + self.transcript.len() as u64 - 1
    }

    fn entry_mut(&mut self, at: u64) -> Option<&mut Entry> {
        let index = usize::try_from(at.checked_sub(self.dropped)?).ok()?;
        self.transcript.get_mut(index)
    }

    /// A prompt went to the agent.
    pub(crate) fn prompted(&mut self, text: &str) {
        self.end_thought(Instant::now());
        self.in_message = false;
        self.turn_start = self.push_entry(EntryKind::User, cut(text, MAX_ENTRY));
        self.turn_links.clear();
        self.turn_images.clear();
        // A tool whose entry left the transcript has nothing left to show.
        let dropped = self.dropped;
        self.tools.retain(|_, tool| tool.entry >= dropped);
        let state = &mut self.info.state;
        state.status = Status::Working;
        state.summary = "Thinking".into();
        state.error = None;
    }

    /// The question the agent asked in `session/request_permission`.
    pub(crate) fn question(&self, id: u32, request: &RequestPermissionRequest) -> Question {
        let call = &request.tool_call;
        let known = self.tools.get(&call.tool_call_id);
        let title = call
            .fields
            .title
            .clone()
            .or_else(|| known.map(|tool| tool.title.clone()))
            .unwrap_or_else(|| "Use a tool".into());
        // A call to Beep's own tools asks in its words, not by the tool's name.
        let beep = BeepTool::of_call(&title, call.meta.as_ref(), call.fields.raw_input.as_ref())
            .or_else(|| known.and_then(|tool| tool.beep.as_ref().map(|beep| beep.tool)));
        let text = match beep {
            Some(tool) => {
                let input = call
                    .fields
                    .raw_input
                    .as_ref()
                    .or_else(|| known.and_then(|tool| tool.beep.as_ref()?.input.as_ref()));
                tool.title(input)
            }
            None => title,
        };
        let options = request
            .options
            .iter()
            .map(|option| AnswerOption {
                id: option.option_id.0.to_string(),
                name: option.name.clone(),
                kind: OptionKind::from_acp(&wire_name(&option.kind)),
            })
            .collect();
        let text = cut(&text, MAX_LINE);
        // Some agents title the question with the command itself.
        let detail = call
            .fields
            .raw_input
            .as_ref()
            .and_then(detail)
            .filter(|detail| *detail != text);
        Question {
            id,
            text,
            detail,
            options,
        }
    }

    /// Applies one `session/update`. True when the compact state changed,
    /// so the server should hear of it; transcript changes alone are not
    /// sent.
    pub(crate) fn apply(&mut self, update: &SessionUpdate) -> bool {
        self.apply_at(update, Instant::now())
    }

    fn apply_at(&mut self, update: &SessionUpdate, now: Instant) -> bool {
        let before = self.info.state.clone();
        let title = self.info.title.clone();
        // Usage, commands and the like arrive in the middle of a thought.
        let ends_thought = matches!(
            update,
            SessionUpdate::AgentMessageChunk(_)
                | SessionUpdate::UserMessageChunk(_)
                | SessionUpdate::ToolCall(_)
                | SessionUpdate::ToolCallUpdate(_)
                | SessionUpdate::Plan(_)
                | SessionUpdate::CompactionUpdate(_)
        );
        if ends_thought {
            self.end_thought(now);
        }
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                ContentBlock::Text(text) => self.message_chunk(&text.text),
                ContentBlock::Image(image) => {
                    if let Some(picture) = image_picture(image) {
                        self.agent_image(picture);
                    }
                }
                ContentBlock::ResourceLink(link) => self.link(&link.uri),
                _ => {}
            },
            // Only a loaded session replays its prompts; a live one has
            // them from `prompted` already.
            SessionUpdate::UserMessageChunk(chunk)
                if self.info.state.status == Status::Starting =>
            {
                match &chunk.content {
                    ContentBlock::Text(text) => {
                        let mut found = Vec::new();
                        let shown = replayed(&text.text, &mut found);
                        for picture in found {
                            self.keep(picture);
                        }
                        self.user_chunk(&shown);
                    }
                    ContentBlock::Image(image) => {
                        if let Ok(data) = STANDARD.decode(&image.data) {
                            self.user_image(Picture {
                                mime: image.mime_type.clone(),
                                data,
                            });
                        }
                    }
                    _ => {}
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => self.thought_chunk(&chunk.content, now),
            SessionUpdate::ToolCall(call) => self.tool_call(call),
            SessionUpdate::ToolCallUpdate(update) => self.tool_call_update(update),
            SessionUpdate::Plan(plan) => {
                self.info.state.plan = plan
                    .entries
                    .iter()
                    .take(MAX_PLAN)
                    .map(|entry| PlanEntry {
                        content: cut(&entry.content, MAX_LINE),
                        status: match entry.status {
                            PlanEntryStatus::InProgress => PlanStatus::InProgress,
                            PlanEntryStatus::Completed => PlanStatus::Completed,
                            _ => PlanStatus::Pending,
                        },
                    })
                    .collect();
            }
            SessionUpdate::SessionInfoUpdate(info) => {
                if let MaybeUndefined::Value(new) = &info.title {
                    self.info.title = Some(cut(new, MAX_LINE));
                }
            }
            SessionUpdate::UsageUpdate(usage) => {
                self.context = Some(Usage {
                    used: usage.used,
                    size: usage.size,
                });
                let cost = usage
                    .cost
                    .as_ref()
                    .map(|cost| (cost.amount, cost.currency.as_str()));
                self.usage_update(cost, usage.meta.as_ref());
            }
            SessionUpdate::AvailableCommandsUpdate(update) => {
                self.commands = update
                    .available_commands
                    .iter()
                    .take(MAX_COMMANDS)
                    .map(|command| cut(&command.name, MAX_LINE))
                    .collect();
            }
            SessionUpdate::CompactionUpdate(update) => self.compaction(update),
            _ => {}
        }
        self.info.state != before || self.info.title != title
    }

    /// Whether the agent compacted the conversation since the last ask.
    pub(crate) fn take_compacted(&mut self) -> bool {
        std::mem::take(&mut self.compacted)
    }

    /// A compaction started or ended: a finished one shows once in the
    /// transcript, and the usage before it no longer holds.
    fn compaction(&mut self, update: &CompactionUpdate) {
        let failed = match &update.status {
            CompactionStatus::InProgress => {
                self.info.state.summary = "Compacting the conversation".into();
                return;
            }
            CompactionStatus::Completed => false,
            CompactionStatus::Failed => true,
            _ => return,
        };
        if self.compactions.len() >= MAX_COMPACTIONS {
            self.compactions.clear();
        }
        if !self.compactions.insert(update.compaction_id.clone()) {
            return;
        }
        if failed {
            let why = match &update.error {
                MaybeUndefined::Value(error) => format!(": {error}"),
                _ => String::new(),
            };
            self.push(
                EntryKind::Other,
                &format!("[compacting the conversation failed{why}]"),
            );
            return;
        }
        self.push(EntryKind::Other, COMPACTED_ENTRY);
        self.context = None;
        self.compacted = true;
    }

    fn user_chunk(&mut self, text: &str) {
        if self.in_message
            && let Some(last) = self.transcript.back_mut()
            && last.kind == EntryKind::User
        {
            push_cut(&mut last.text, text, MAX_ENTRY);
        } else {
            self.push_entry(EntryKind::User, cut(text, MAX_ENTRY));
            self.in_message = true;
            // A replayed prompt starts a turn.
            self.turn_images.clear();
        }
    }

    /// An image the agent replays with a prompt, on its own line.
    fn user_image(&mut self, picture: Picture) {
        let line = picture.line();
        self.keep(picture);
        let joined = match self.transcript.back() {
            Some(last) if self.in_message && last.kind == EntryKind::User => format!("\n{line}"),
            _ => line,
        };
        self.user_chunk(&joined);
    }

    /// An image the agent shows in its message, on its own line.
    fn agent_image(&mut self, picture: Picture) {
        let line = picture.line();
        self.keep(picture);
        // Whole, and not in the last message the server is told.
        if self.in_message
            && let Some(last) = self.transcript.back_mut()
            && last.kind == EntryKind::Agent
        {
            last.text.push('\n');
            last.text.push_str(&line);
            last.text.push('\n');
        } else {
            self.push_entry(EntryKind::Agent, format!("{line}\n"));
            self.in_message = true;
            self.info.last_message.clear();
        }
    }

    /// Pictures a tool call showed, each also on an agent line of its own
    /// so the transcript shows it with the tools folded: once a turn, at
    /// most `image_files::MAX_PER_TURN`, and none too large to send.
    fn tool_images(&mut self, pictures: &[Picture]) {
        for picture in pictures {
            if self.turn_images.len() >= image_files::MAX_PER_TURN {
                return;
            }
            let line = picture.line();
            if picture.data.len() > image_files::MAX_FILE || self.turn_images.contains(&line) {
                continue;
            }
            // `tool_output` kept it already.
            self.push_entry(EntryKind::Agent, format!("{line}\n"));
            self.in_message = false;
            self.turn_images.push(line);
        }
    }

    /// A file the agent links to: an image one shows when the turn ends.
    fn link(&mut self, uri: &str) {
        if self.turn_links.len() < image_files::MAX_PER_TURN {
            self.turn_links
                .extend(image_files::named_in(uri).map(str::to_owned));
        }
    }

    /// The image files the turn that just ended named: in the agent's
    /// messages, its links, the files its finished edits wrote, and the
    /// ones its finished reads looked at without showing them. Files a
    /// read showed as pictures are left out, and so is the room those
    /// pictures took. `None` when there are none.
    pub(crate) fn turn_files(&mut self) -> Option<TurnFiles> {
        let cwd = PathBuf::from(&self.info.cwd);
        let start = self.turn_start;
        let mut entry = None;
        let mut named: Vec<String> = Vec::new();
        for (at, item) in (self.dropped..).zip(&self.transcript) {
            if at > start && item.kind == EntryKind::Agent {
                entry = Some(at);
                named.extend(image_files::named_in(&item.text).map(str::to_owned));
            }
        }
        named.append(&mut self.turn_links);
        let mut done: Vec<&ToolCall> = self
            .tools
            .values()
            .filter(|tool| tool.entry > start && tool.status == ToolCallStatus::Completed)
            .collect();
        done.sort_by_key(|tool| tool.entry);
        let mut shown: Vec<PathBuf> = Vec::new();
        for tool in done {
            let read = tool.kind == ToolKind::Read;
            if read && !tool.pictures.is_empty() {
                shown.extend(
                    tool.paths
                        .iter()
                        .filter_map(|name| image_files::resolve(&cwd, name)),
                );
            } else if read || EDIT_KINDS.contains(&tool.kind) {
                named.extend(tool.paths.iter().cloned());
            }
        }
        let room = image_files::MAX_PER_TURN.saturating_sub(self.turn_images.len());
        let mut paths: Vec<PathBuf> = Vec::new();
        for path in named
            .iter()
            .filter_map(|name| image_files::resolve(&cwd, name))
        {
            if paths.len() >= room {
                break;
            }
            if !paths.contains(&path) && !shown.contains(&path) {
                paths.push(path);
            }
        }
        (!paths.is_empty()).then_some(TurnFiles { cwd, paths, entry })
    }

    /// Shows `pictures`, read from the files `turn_files` gave, after the
    /// agent message `entry`: each on its own line, once.
    pub(crate) fn show_files(&mut self, entry: Option<u64>, pictures: Vec<Picture>) {
        let mut lines = String::new();
        for picture in pictures {
            let line = picture.line();
            self.keep(picture);
            let shown = self.turn_images.contains(&line)
                || entry
                    .and_then(|at| self.entry_mut(at))
                    .is_some_and(|item| item.text.contains(&line));
            if !shown && !lines.contains(&line) {
                lines.push('\n');
                lines.push_str(&line);
            }
        }
        if lines.is_empty() {
            return;
        }
        // The lines go whole, past an entry's length if need be.
        match entry.and_then(|at| self.entry_mut(at)) {
            Some(item) => item.text.push_str(&lines),
            None => {
                self.push_entry(EntryKind::Agent, lines.trim_start().to_owned());
                self.in_message = false;
            }
        }
    }

    /// Part of a thought: the summary says "Thinking" until the thought has
    /// a line to show, then that line. The thought is a transcript entry of
    /// its own once it has text, so a redacted one shows nothing.
    fn thought_chunk(&mut self, content: &ContentBlock, now: Instant) {
        if !self.in_thought {
            self.in_thought = true;
            self.thought.clear();
            self.info.state.summary = "Thinking".into();
            // A loaded session replays its thoughts all at once.
            let started = (self.info.state.status != Status::Starting).then_some(now);
            self.open_thought = Some(OpenThought {
                entry: None,
                started,
            });
        }
        if let ContentBlock::Text(text) = content {
            if self.thought.len() < MAX_THOUGHT {
                self.thought.push_str(&text.text);
            }
            self.thought_text(&text.text);
        }
        if let Some(line) = thought_line(&self.thought) {
            self.info.state.summary = cut(&line, MAX_LINE);
        }
    }

    /// Adds `text` to the open thought's entry, made at its first text.
    fn thought_text(&mut self, text: &str) {
        let Some(open) = &self.open_thought else {
            return;
        };
        match open.entry {
            Some(at) => {
                if let Some(entry) = self.entry_mut(at) {
                    push_cut(&mut entry.text, text, MAX_ENTRY);
                }
            }
            None if !text.trim().is_empty() => {
                self.in_message = false;
                let at = self.push_entry(EntryKind::Thought, cut(text.trim_start(), MAX_ENTRY));
                if let Some(open) = &mut self.open_thought {
                    open.entry = Some(at);
                }
            }
            None => {}
        }
    }

    /// The thought arriving ended at `now`: its entry says how long it took.
    pub(crate) fn end_thought(&mut self, now: Instant) {
        self.in_thought = false;
        let Some(open) = self.open_thought.take() else {
            return;
        };
        let Some(entry) = open.entry.and_then(|at| self.entry_mut(at)) else {
            return;
        };
        entry.text.truncate(entry.text.trim_end().len());
        if let Some(started) = open.started {
            let took = now.saturating_duration_since(started).as_millis();
            entry.ms = Some(u64::try_from(took).unwrap_or(u64::MAX));
        }
    }

    fn message_chunk(&mut self, text: &str) {
        if self.in_message
            && let Some(last) = self.transcript.back_mut()
            && last.kind == EntryKind::Agent
        {
            push_cut(&mut last.text, text, MAX_ENTRY);
        } else {
            self.push_entry(EntryKind::Agent, cut(text, MAX_ENTRY));
            self.in_message = true;
            self.info.last_message.clear();
        }
        push_cut(&mut self.info.last_message, text, MAX_LAST_MESSAGE);
    }

    fn tool_call(&mut self, call: &AcpToolCall) {
        self.in_message = false;
        // Codex saying a subagent it started began or ended goes on the
        // call that started it.
        if let Some(activity) = call.raw_input.as_ref().and_then(Activity::of)
            && let Some(owner) = self.codex_threads.get(&activity.thread).cloned()
        {
            self.codex_activity(&owner, activity);
            return;
        }
        // A subagent's own call goes on the entry of the call that started
        // it, not in the transcript.
        let parent = parent_tool(call.meta.as_ref()).filter(|id| self.tools.contains_key(id));
        let mut subagent = Subagent::of_call(&call.title, call.raw_input.as_ref());
        let mut tool = ToolCall {
            entry: 0,
            title: call.title.clone(),
            kind: call.kind,
            status: call.status,
            paths: paths(&call.locations, call.raw_input.as_ref()),
            output: String::new(),
            pictures: Vec::new(),
            parent: parent.clone(),
            steps: Vec::new(),
            subagent: None,
            beep: BeepTool::of_call(&call.title, call.meta.as_ref(), call.raw_input.as_ref()).map(
                |tool| BeepCall {
                    tool,
                    input: call.raw_input.clone(),
                },
            ),
        };
        if let Some(subagent) = &mut subagent {
            if let Some(title) = subagent.title.take() {
                tool.title = title;
            }
            for thread in &subagent.threads {
                self.codex_threads
                    .insert(thread.clone(), call.tool_call_id.clone());
            }
        }
        tool.subagent = subagent;
        let pictures = self.tool_output(&mut tool, &call.content);
        self.info.state.summary = cut(&tool.own_title(), MAX_LINE);
        self.finish_tool(&tool);
        match parent {
            Some(parent) => {
                if let Some(owner) = self.tools.get_mut(&parent) {
                    tool.entry = owner.entry;
                    owner.steps.push(call.tool_call_id.clone());
                }
                self.tools.insert(call.tool_call_id.clone(), tool);
                self.write_tool(&parent);
            }
            None => {
                tool.entry = self.push_entry(EntryKind::Tool, tool.entry_text());
                self.tools.insert(call.tool_call_id.clone(), tool);
            }
        }
        self.tool_images(&pictures);
    }

    /// Codex's report that the subagent `owner` started began or ended:
    /// its task names it.
    fn codex_activity(&mut self, owner: &ToolCallId, activity: Activity) {
        if let Some(tool) = self.tools.get_mut(owner) {
            tool.title = activity.task;
            if let Some(subagent) = &mut tool.subagent {
                subagent.ended |= activity.ended;
            }
        }
        self.write_tool(owner);
    }

    /// The calls the subagent `tool` started ran, as its entry names them.
    fn steps_of<'a>(&'a self, tool: &'a ToolCall) -> Vec<Step<'a>> {
        tool.steps
            .iter()
            .filter_map(|step| self.tools.get(step))
            .map(|step| Step {
                status: step.status,
                title: &step.title,
            })
            .collect()
    }

    /// Writes tool `id`'s entry again: its own text, and the steps of the
    /// subagent it started.
    fn write_tool(&mut self, id: &ToolCallId) {
        let Some(tool) = self.tools.get(id) else {
            return;
        };
        let text = tool.entry_with(&self.steps_of(tool), None);
        let entry = tool.entry;
        if let Some(entry) = self.entry_mut(entry) {
            entry.text = text;
        }
    }

    fn tool_call_update(&mut self, update: &ToolCallUpdate) {
        let Some(mut call) = self.tools.remove(&update.tool_call_id) else {
            return;
        };
        let fields = &update.fields;
        let codex = call.subagent.as_ref().is_some_and(|sub| sub.codex);
        if let Some(title) = &fields.title {
            // A Codex subagent keeps the title its task or prompt gave it.
            if !codex {
                call.title.clone_from(title);
            }
            self.info.state.summary = cut(title, MAX_LINE);
        }
        // Claude Code sends a call's arguments once they have all come.
        if call.beep.is_none() {
            call.beep =
                BeepTool::of_call(&call.title, update.meta.as_ref(), fields.raw_input.as_ref())
                    .map(|tool| BeepCall { tool, input: None });
        }
        if let Some(beep) = &mut call.beep {
            if let Some(input) = &fields.raw_input {
                beep.input = Some(input.clone());
            }
            self.info.state.summary = cut(&call.own_title(), MAX_LINE);
        }
        if let Some(subagent) = call.subagent.as_mut().filter(|sub| sub.codex) {
            subagent.update(fields.raw_input.as_ref());
            for thread in &subagent.threads {
                self.codex_threads
                    .insert(thread.clone(), update.tool_call_id.clone());
            }
        }
        if let Some(kind) = fields.kind {
            call.kind = kind;
        }
        if let Some(status) = fields.status {
            call.status = status;
        }
        let more = paths(
            fields.locations.as_deref().unwrap_or_default(),
            fields.raw_input.as_ref(),
        );
        if !more.is_empty() {
            call.paths = more;
        }
        let pictures = match &fields.content {
            Some(content) => self.tool_output(&mut call, content),
            None => Vec::new(),
        };
        self.finish_tool(&call);
        let shown = call
            .parent
            .clone()
            .unwrap_or_else(|| update.tool_call_id.clone());
        self.tools.insert(update.tool_call_id.clone(), call);
        self.write_tool(&shown);
        self.tool_images(&pictures);
    }

    /// Sets what `tool` showed: its text, and a line for each picture,
    /// kept for the server. Returns the pictures, for `tool_images`.
    fn tool_output(&mut self, tool: &mut ToolCall, content: &[ToolCallContent]) -> Vec<Picture> {
        let shown = output(content, Path::new(&self.info.cwd));
        tool.output = shown.text;
        tool.pictures = shown.pictures.iter().map(Picture::line).collect();
        for picture in &shown.pictures {
            self.keep(picture.clone());
        }
        for link in &shown.links {
            self.link(link);
        }
        shown.pictures
    }

    /// Notes the files a finished edit changed.
    fn finish_tool(&mut self, tool: &ToolCall) {
        if tool.status != ToolCallStatus::Completed || !EDIT_KINDS.contains(&tool.kind) {
            return;
        }
        let cwd = Path::new(&self.info.cwd);
        let files = &mut self.info.state.files_touched;
        for path in &tool.paths {
            let shown = Path::new(path)
                .strip_prefix(cwd)
                .map_or_else(|_| path.clone(), |rel| rel.display().to_string());
            files.retain(|file| *file != shown);
            files.push(shown);
        }
        let extra = files.len().saturating_sub(MAX_FILES);
        files.drain(..extra);
    }
}

/// One call a subagent ran, as its parent's entry names it.
struct Step<'a> {
    status: ToolCallStatus,
    title: &'a str,
}

impl ToolCall {
    /// Its title and status, what it showed, then its image lines whole.
    fn entry_text(&self) -> String {
        self.entry_with(&[], None)
    }

    /// How the call stands as its entry shows it: a Codex subagent works
    /// until it says it ended, or its rollout says it finished.
    fn shown_status(&self, run: Option<&Run>) -> ToolCallStatus {
        let codex = self.subagent.as_ref().filter(|sub| sub.codex);
        if self.status == ToolCallStatus::Failed || codex.is_none() {
            return self.status;
        }
        let working = match run {
            Some(run) => run.working,
            None => codex.is_some_and(|sub| !sub.ended),
        };
        if working {
            ToolCallStatus::InProgress
        } else {
            ToolCallStatus::Completed
        }
    }

    /// Its title as its entry shows it. Claude Code's `Artifact` call comes
    /// titled by the tool's name alone, so the page it publishes is added
    /// for the apps' Artifacts tab: `Artifact /srv/project/report.html`.
    fn own_title(&self) -> Cow<'_, str> {
        if let Some(beep) = &self.beep {
            return Cow::Owned(beep.tool.title(beep.input.as_ref()));
        }
        match self.paths.first() {
            Some(path) if self.title == ARTIFACT_TOOL => {
                Cow::Owned(format!("{} {path}", self.title))
            }
            _ => Cow::Borrowed(&self.title),
        }
    }

    /// Its entry with the steps of the subagent it started: a line
    /// `[steps] N`, then `[step <status>] <title>` for each of the last
    /// `MAX_STEPS`, then `[agent] <name>` when it has one, before what it
    /// showed. A Codex subagent's steps and answer are `run`'s, read from
    /// its rollout.
    fn entry_with(&self, steps: &[Step<'_>], run: Option<&Run>) -> String {
        let mut lines = String::new();
        for line in &self.pictures {
            lines.push('\n');
            lines.push_str(line);
        }
        let status = self.shown_status(run);
        let own = self.own_title();
        let title = run.and_then(|run| run.task.as_deref()).unwrap_or(&own);
        let mut text = format!("{title} [{}]", wire_name(&status));
        let run_steps: Vec<Step<'_>> = run
            .map(|run| {
                run.steps
                    .iter()
                    .map(|step| Step {
                        status: if step.done || status != ToolCallStatus::InProgress {
                            ToolCallStatus::Completed
                        } else {
                            ToolCallStatus::InProgress
                        },
                        title: &step.title,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (steps, count) = match run {
            Some(run) => (run_steps.as_slice(), run.count),
            None => (steps, steps.len()),
        };
        if !steps.is_empty() || self.subagent.is_some() {
            text.push_str(&format!("\n[steps] {count}"));
            for step in &steps[steps.len().saturating_sub(MAX_STEPS)..] {
                let title = step.title.lines().next().unwrap_or_default();
                text.push_str(&format!(
                    "\n[step {}] {}",
                    wire_name(&step.status),
                    cut(title, MAX_LINE)
                ));
            }
            let name = run
                .and_then(|run| run.name.as_deref())
                .or(self.subagent.as_ref().and_then(|sub| sub.name.as_deref()));
            if let Some(name) = name {
                text.push_str(&format!("\n[agent] {name}"));
            }
        }
        let output = run
            .and_then(|run| run.result.as_deref())
            .unwrap_or(&self.output);
        if !output.is_empty() {
            text.push('\n');
            text.push_str(output);
        }
        let mut text = cut(&text, MAX_ENTRY.saturating_sub(lines.len()));
        text.push_str(&lines);
        text
    }
}

/// What a tool call's `content` shows.
struct Output {
    /// Its text, cut to an entry's length.
    text: String,
    /// The images among it.
    pictures: Vec<Picture>,
    /// The files it links to.
    links: Vec<String>,
}

/// What a tool call's `content` shows: its text, images and links. An
/// edit's diff shows its changed lines, its path relative to `cwd`.
fn output(content: &[ToolCallContent], cwd: &Path) -> Output {
    let mut pictures = Vec::new();
    let mut links = Vec::new();
    let text = content
        .iter()
        .filter_map(|item| match item {
            ToolCallContent::Content(content) => match &content.content {
                ContentBlock::Text(text) => Some(text.text.clone()),
                ContentBlock::Image(image) => {
                    pictures.extend(image_picture(image));
                    None
                }
                ContentBlock::Resource(resource) => {
                    if let EmbeddedResourceResource::BlobResourceContents(blob) = &resource.resource
                    {
                        let mime = blob.mime_type.as_deref().unwrap_or("");
                        pictures.extend(picture_of(mime, &blob.blob));
                    }
                    None
                }
                ContentBlock::ResourceLink(link) => {
                    links.push(link.uri.clone());
                    None
                }
                _ => None,
            },
            ToolCallContent::Diff(diff) => Some(diff_text(
                cwd,
                &diff.path,
                diff.old_text.as_deref(),
                &diff.new_text,
            )),
            ToolCallContent::Terminal(_) => Some("(ran in a terminal)".into()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    Output {
        text: cut(&text, MAX_ENTRY),
        pictures,
        links,
    }
}

/// The picture of an ACP image block.
fn image_picture(image: &ImageContent) -> Option<Picture> {
    picture_of(&image.mime_type, &image.data)
}

/// The picture `data`, in base64, holds when `mime` is an image type.
fn picture_of(mime: &str, data: &str) -> Option<Picture> {
    if !mime.starts_with("image/") {
        return None;
    }
    let data = STANDARD.decode(data.trim()).ok()?;
    Some(Picture {
        mime: mime.to_owned(),
        data,
    })
}

/// The files a tool call names, in `locations` or its input.
fn paths(locations: &[ToolCallLocation], input: Option<&Value>) -> Vec<String> {
    let mut found: Vec<String> = locations
        .iter()
        .map(|location| location.path.display().to_string())
        .collect();
    if found.is_empty()
        && let Some(input) = input
    {
        found.extend(
            ["file_path", "path", "notebook_path"]
                .iter()
                .find_map(|key| input[key].as_str())
                .map(str::to_owned),
        );
    }
    found
}

/// What a permission question would act on: the command, the file, or the
/// input itself.
fn detail(input: &Value) -> Option<String> {
    let text = ["command", "file_path", "path", "url"]
        .iter()
        .find_map(|key| input[key].as_str())
        .map(str::to_owned)
        .or_else(|| (!input.is_null()).then(|| input.to_string()))?;
    Some(cut(&text, MAX_LINE))
}

/// `text` cut to at most `max` bytes, on a character boundary.
pub(crate) fn cut(text: &str, max: usize) -> String {
    let mut out = String::new();
    push_cut(&mut out, text, max);
    out
}

fn push_cut(out: &mut String, text: &str, max: usize) {
    let room = max.saturating_sub(out.len());
    if text.len() <= room {
        out.push_str(text);
        return;
    }
    let mut end = room;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&text[..end]);
}

/// `text` as an agent replays it, with each image it inlined as a markdown
/// link to a data URL, `[@name](data:image/png;base64,...)`, made an image
/// line of its own. The pictures go in `found`.
fn replayed(text: &str, found: &mut Vec<Picture>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("](data:image/") {
        let url_at = at + 2;
        let (Some(open), Some(len)) = (rest[..at].rfind('['), rest[url_at..].find(')')) else {
            break;
        };
        let picture = data_url(&rest[url_at..url_at + len]);
        let end = url_at + len + 1;
        match picture {
            Some(picture) => {
                out.push_str(rest[..open].trim_end_matches(' '));
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&picture.line());
                found.push(picture);
                if !rest[end..].is_empty() && !rest[end..].starts_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str(&rest[..end]),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The subagent tool call a call ran in, as Claude Code's adapter stamps
/// it (`_meta.claudeCode.parentToolUseId`).
fn parent_tool(meta: Option<&serde_json::Map<String, Value>>) -> Option<ToolCallId> {
    let id = meta?.get("claudeCode")?.get("parentToolUseId")?.as_str()?;
    Some(ToolCallId::new(id.to_owned()))
}

/// The line a thought shows while it arrives: its last bold heading
/// (`**Planning the fix**`), else its first sentence once that is whole.
/// `None` until there is one.
fn thought_line(text: &str) -> Option<String> {
    let mut heading = None;
    let mut rest = text;
    while let Some(start) = rest.find("**") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("**") else {
            break;
        };
        let bold = after[..end].trim();
        if !bold.is_empty() && !bold.contains('\n') {
            heading = Some(bold);
        }
        rest = &after[end + 2..];
    }
    if let Some(heading) = heading {
        return Some(heading.to_owned());
    }
    let text = text.trim_start();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let end = match c {
            '\n' => Some(at),
            '.' | '!' | '?' if chars.peek().is_some_and(|(_, next)| next.is_whitespace()) => {
                Some(at + c.len_utf8())
            }
            _ => None,
        };
        if let Some(end) = end {
            let line: String = text[..end]
                .chars()
                .filter(|c| !matches!(c, '*' | '`'))
                .collect();
            let line = line.trim();
            return (!line.is_empty()).then(|| line.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::subagent::RunStep;
    use beep_proto::image_key;
    use futures::FutureExt;
    use serde_json::json;
    use std::time::Duration;

    fn update(value: Value) -> SessionUpdate {
        serde_json::from_value(value).expect("a valid update")
    }

    fn permission(value: Value) -> RequestPermissionRequest {
        serde_json::from_value(value).expect("a valid permission request")
    }

    fn live() -> Live {
        Live::new(
            SessionInfo {
                id: "a3f9c1".into(),
                cwd: "/srv/project".into(),
                ..SessionInfo::default()
            },
            Arc::default(),
            Arc::default(),
        )
    }

    fn thought(text: &str) -> SessionUpdate {
        update(json!({"sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": text}}))
    }

    fn said(text: &str) -> SessionUpdate {
        update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": text}}))
    }

    #[test]
    fn a_thought_is_an_entry_that_says_how_long_it_took() {
        let mut live = live();
        live.prompted("fix it");
        let start = Instant::now();
        live.apply_at(&thought("**Reading**\n\nThe test "), start);
        live.apply_at(&thought("expects the old text."), start);
        // Usage in the middle does not split it.
        live.apply_at(
            &update(json!({"sessionUpdate": "usage_update", "used": 10, "size": 100})),
            start + Duration::from_secs(3),
        );
        live.apply_at(&thought(" More."), start + Duration::from_secs(5));
        let (entries, _) = live.transcript(10);
        assert_eq!(entries[1].kind, EntryKind::Thought);
        assert_eq!(entries[1].ms, None, "not over yet");
        live.apply_at(&said("Fixed."), start + Duration::from_millis(12_400));
        live.apply_at(&thought("Now the docs.\n"), start + Duration::from_secs(13));
        live.end_thought(start + Duration::from_secs(15));
        let (entries, _) = live.transcript(10);
        let kinds: Vec<_> = entries.iter().map(|entry| entry.kind).collect();
        assert_eq!(
            kinds,
            [
                EntryKind::User,
                EntryKind::Thought,
                EntryKind::Agent,
                EntryKind::Thought
            ]
        );
        assert_eq!(
            entries[1].text,
            "**Reading**\n\nThe test expects the old text. More."
        );
        assert_eq!(entries[1].ms, Some(12_400));
        assert_eq!(entries[2].text, "Fixed.");
        assert_eq!(entries[3].text, "Now the docs.");
        assert_eq!(entries[3].ms, Some(2_000));
    }

    #[test]
    fn a_thought_without_text_or_replayed_has_no_entry_or_time() {
        let mut live = live();
        let start = Instant::now();
        // A loaded session replays a thought: its time is not known.
        live.apply_at(&thought("Earlier thinking."), start);
        live.apply_at(&said("Earlier answer."), start + Duration::from_secs(9));
        live.prompted("go on");
        live.apply_at(&thought(""), start);
        live.apply_at(&said("Done."), start + Duration::from_secs(4));
        let (entries, _) = live.transcript(10);
        let kinds: Vec<_> = entries.iter().map(|entry| entry.kind).collect();
        assert_eq!(
            kinds,
            [
                EntryKind::Thought,
                EntryKind::Agent,
                EntryKind::User,
                EntryKind::Agent
            ]
        );
        assert_eq!(entries[0].ms, None);
    }

    #[test]
    fn message_chunks_join_into_one_entry() {
        let mut live = live();
        live.prompted("hi");
        let chunk = |text: &str| {
            update(
                json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}),
            )
        };
        assert!(!live.apply(&chunk("Hel")), "chunks alone are not sent");
        live.apply(&chunk("lo"));
        assert_eq!(live.info.last_message, "Hello");
        let (entries, earlier) = live.transcript(10);
        assert_eq!(earlier, 0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].text, "Hello");
        live.push(EntryKind::User, "again");
        live.apply(&chunk("New"));
        assert_eq!(live.info.last_message, "New", "a new message starts over");
        assert_eq!(live.transcript(1).1, 3);
    }

    #[test]
    fn a_loaded_session_replays_its_prompts_and_answers() {
        let mut live = live();
        let chunk = |kind: &str, text: &str| {
            update(json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}}))
        };
        live.apply(&chunk("user_message_chunk", "Fix the "));
        live.apply(&chunk("user_message_chunk", "build"));
        live.apply(&chunk("agent_message_chunk", "Fixed."));
        let (entries, _) = live.transcript(10);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, EntryKind::User);
        assert_eq!(entries[0].text, "Fix the build");
        assert_eq!(entries[1].text, "Fixed.");
        // Once running, prompts come from `prompted`, not the agent's echo.
        live.prompted("again");
        live.apply(&chunk("user_message_chunk", "again"));
        assert_eq!(live.transcript(10).0.len(), 3);
    }

    #[test]
    fn a_replayed_image_becomes_an_image_line() {
        let mut live = live();
        let data = STANDARD.encode(b"\x89PNG");
        let line = format!("[image: image-{}]", image_key(b"\x89PNG"));
        live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": format!("what is this[@image](data:image/png;base64,{data})")}})));
        live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
            "content": {"type": "image", "mimeType": "image/png", "data": data}})));
        let (entries, _) = live.transcript(10);
        assert_eq!(entries[0].text, format!("what is this\n{line}\n{line}"));
        let mut found = Vec::new();
        assert_eq!(
            replayed("a [link](https://x.dev) stays", &mut found),
            "a [link](https://x.dev) stays"
        );
        assert!(found.is_empty());
    }

    #[test]
    fn a_replayed_image_is_kept_by_its_key() {
        let mut live = live();
        let png = b"\x89PNG\r\n\x1a\nshot".to_vec();
        let url = format!("data:image/png;base64,{}", STANDARD.encode(&png));
        live.apply(&update(json!({"sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": format!("look [@shot.png]({url})")}})));
        let kept = live.images.get("a3f9c1", &image_key(&png));
        assert_eq!(
            kept.as_deref(),
            Some(&Picture {
                mime: "image/png".into(),
                data: png,
            })
        );
    }

    #[test]
    fn tool_calls_go_with_their_entries() {
        let mut live = live();
        for n in 0..MAX_ENTRIES {
            live.prompted("again");
            live.apply(&update(json!({
                "sessionUpdate": "tool_call",
                "toolCallId": format!("t{n}"),
                "title": "Run the tests",
                "kind": "execute",
                "status": "completed",
            })));
        }
        live.prompted("once more");
        assert!(
            live.tools.len() <= MAX_ENTRIES / 2 + 1,
            "{}",
            live.tools.len()
        );
    }

    #[test]
    fn an_edit_shows_its_diff() {
        let mut live = live();
        live.prompted("fix it");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "e1",
            "title": "Edit `src/login.rs`",
            "kind": "edit",
            "status": "completed",
            "content": [{"type": "diff", "path": "/srv/project/src/login.rs",
                "oldText": "fn a() {\n    old();\n}", "newText": "fn a() {\n    new();\n}"}]
        })));
        let (entries, _) = live.transcript(1);
        assert_eq!(
            entries[0].text,
            "Edit `src/login.rs` [completed]\n[diff] src/login.rs +1 -1\n fn a() {\n-    old();\n+    new();\n }"
        );
    }

    #[test]
    fn a_tool_result_image_shows_on_its_tool_and_goes_to_the_server() {
        let mut live = live();
        live.prompted("take a screenshot");
        let png = b"\x89PNG\r\n\x1a\nscreen".to_vec();
        live.apply(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Screenshot",
            "kind": "other",
            "status": "completed",
            "content": [
                {"type": "content", "content": {"type": "text", "text": "taken"}},
                {"type": "content", "content": {"type": "image", "mimeType": "image/png",
                    "data": STANDARD.encode(&png)}}
            ]
        })));
        let line = format!("[image: image-{}]", image_key(&png));
        let (entries, _) = live.transcript(2);
        assert_eq!(
            entries[0].text,
            format!("Screenshot [completed]\ntaken\n{line}")
        );
        assert_eq!(entries[1].kind, EntryKind::Agent);
        assert_eq!(
            entries[1].text,
            format!("{line}\n"),
            "shown with the tools folded"
        );
        assert!(live.picture(&image_key(&png)).is_some());
        let sent = live.images.next_unsent().now_or_never().expect("one waits");
        assert_eq!(sent.session, "a3f9c1");
        assert_eq!(sent.picture.data, png);
        // The same picture again is not sent twice.
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "content": [{"type": "content", "content": {"type": "image", "mimeType": "image/png",
                "data": STANDARD.encode(&png)}}]
        })));
        assert!(live.images.next_unsent().now_or_never().is_none());
    }

    #[test]
    fn an_image_in_the_agents_message_gets_its_own_line() {
        let mut live = live();
        live.prompted("draw");
        let png = b"\x89PNG\r\n\x1a\ndrawn".to_vec();
        live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Here it is:"}})));
        live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "image", "mimeType": "image/png", "data": STANDARD.encode(&png)}})));
        let (entries, _) = live.transcript(1);
        let line = format!("[image: image-{}]", image_key(&png));
        assert_eq!(entries[0].text, format!("Here it is:\n{line}\n"));
    }

    #[test]
    fn a_turn_names_the_image_files_it_mentioned_and_wrote() {
        let mut live = live();
        live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Earlier: old.png"}})));
        live.prompted("plot it");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "w1",
            "title": "Write plots/b.svg",
            "kind": "edit",
            "status": "completed",
            "locations": [{"path": "/srv/project/plots/b.svg"}]
        })));
        live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Saved `plots/a.png` and plots/b.svg."}})));
        let files = live.turn_files().expect("files named");
        assert_eq!(files.cwd, Path::new("/srv/project"));
        assert_eq!(
            files.paths,
            [
                PathBuf::from("/srv/project/plots/a.png"),
                PathBuf::from("/srv/project/plots/b.svg")
            ]
        );
        assert_eq!(files.entry, Some(3));
        let png = b"\x89PNG\r\n\x1a\nplot".to_vec();
        let picture = Picture {
            mime: "image/png".into(),
            data: png.clone(),
        };
        live.show_files(files.entry, vec![picture.clone(), picture]);
        let line = format!("[image: image-{}]", image_key(&png));
        let (entries, _) = live.transcript(1);
        assert_eq!(
            entries[0].text,
            format!("Saved `plots/a.png` and plots/b.svg.\n{line}"),
            "once"
        );
        live.prompted("nothing");
        assert_eq!(live.turn_files(), None);
    }

    #[test]
    fn an_artifact_call_names_the_page_it_publishes() {
        let mut live = live();
        live.apply(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "a1",
            "title": "Artifact",
            "kind": "other",
            "status": "in_progress",
            "rawInput": {"file_path": "/srv/project/report.html"}
        })));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "a1",
            "status": "completed",
            "content": [{"type": "content", "content": {"type": "text",
                "text": "Published https://claude.ai/code/artifact/0000-placeholder"}}]
        })));
        let (entries, _) = live.transcript(1);
        assert_eq!(
            entries[0].text,
            "Artifact /srv/project/report.html [completed]\nPublished https://claude.ai/code/artifact/0000-placeholder"
        );
    }

    #[test]
    fn a_beep_call_is_titled_from_its_arguments() {
        let mut live = live();
        // Claude Code names the tool, then sends the arguments once complete.
        live.apply(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "b1",
            "title": "mcp__beep__run_on",
            "kind": "other",
            "status": "pending",
            "rawInput": {},
            "_meta": {"claudeCode": {"toolName": "mcp__beep__run_on"}}
        })));
        let (entries, _) = live.transcript(1);
        assert_eq!(entries[0].text, "Beep: Run on a machine [pending]");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "b1",
            "title": "mcp__beep__run_on",
            "rawInput": {"machine": "build-box", "command": "cargo test"}
        })));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "b1",
            "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "exit 0"}}]
        })));
        let (entries, _) = live.transcript(1);
        assert_eq!(
            entries[0].text,
            "Beep: Run on build-box: cargo test [completed]\nexit 0"
        );
        assert_eq!(
            live.info.state.summary,
            "Beep: Run on build-box: cargo test"
        );
    }

    /// A Read tool call of `path`, as Claude Code's ACP adapter sends it.
    fn read_call(id: &str, path: &str) -> SessionUpdate {
        update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": format!("Read {path}"),
            "kind": "read",
            "status": "pending",
            "locations": [{"path": path, "line": 1}],
            "rawInput": {"file_path": path}
        }))
    }

    /// Its result: `png` as an image block, or plain text without one.
    fn read_done(id: &str, png: Option<&[u8]>) -> SessionUpdate {
        let content = match png {
            Some(png) => json!({"type": "image", "mimeType": "image/png",
                "data": STANDARD.encode(png)}),
            None => json!({"type": "text", "text": "(binary)"}),
        };
        update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": "completed",
            "content": [{"type": "content", "content": content}]
        }))
    }

    fn agent_lines(live: &Live) -> Vec<String> {
        live.transcript(MAX_ENTRIES)
            .0
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::Agent)
            .map(|entry| entry.text)
            .collect()
    }

    #[test]
    fn an_image_a_read_shows_is_an_agent_image() {
        let mut live = live();
        live.prompted("draw the parts");
        let png = b"\x89PNG\r\n\x1a\nparts".to_vec();
        live.apply(&read_call("r1", "/srv/project/sketches/parts.png"));
        live.apply(&read_done("r1", Some(&png)));
        let line = format!("[image: image-{}]", image_key(&png));
        assert_eq!(agent_lines(&live), [format!("{line}\n")]);
        let (entries, _) = live.transcript(2);
        assert_eq!(entries[0].kind, EntryKind::Tool);
        assert!(live.picture(&image_key(&png)).is_some());
        // The file it read is not read again from the disk, even named.
        live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Drew sketches/parts.png."}})));
        assert_eq!(live.turn_files(), None);
    }

    #[test]
    fn an_image_file_a_read_named_shows_when_inside_the_folder() {
        let root = std::env::temp_dir().join(format!("beep-live-reads-{}", std::process::id()));
        let cwd = root.join("project");
        std::fs::create_dir_all(cwd.join("sketches")).unwrap();
        let png = b"\x89PNG\r\n\x1a\nlineup".to_vec();
        std::fs::write(cwd.join("sketches/lineup.png"), &png).unwrap();
        std::fs::write(root.join("outside.png"), b"\x89PNG\r\n\x1a\nout").unwrap();
        let mut live = Live::new(
            SessionInfo {
                id: "b7e2d0".into(),
                cwd: cwd.display().to_string(),
                ..SessionInfo::default()
            },
            Arc::default(),
            Arc::default(),
        );
        live.prompted("look at them");
        let inside = cwd.join("sketches/lineup.png").display().to_string();
        let outside = root.join("outside.png").display().to_string();
        live.apply(&read_call("r1", &inside));
        live.apply(&read_done("r1", None));
        live.apply(&read_call("r2", &outside));
        live.apply(&read_done("r2", None));
        // Still running: its file is not read.
        live.apply(&read_call("r3", "notes.png"));
        live.apply(&update(json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "Looked."}})));
        let files = live.turn_files().expect("files named");
        let pictures = image_files::read(&files.cwd, &files.paths);
        assert_eq!(pictures.len(), 1, "only the one inside the folder");
        assert_eq!(pictures[0].data, png);
        live.show_files(files.entry, pictures);
        let line = format!("[image: image-{}]", image_key(&png));
        assert_eq!(agent_lines(&live), [format!("Looked.\n{line}")]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_turn_shows_each_tool_image_once_and_a_bounded_few() {
        let mut live = live();
        live.prompted("look twice");
        let png = b"\x89PNG\r\n\x1a\nsame".to_vec();
        live.apply(&read_call("r1", "/srv/project/a.png"));
        live.apply(&read_done("r1", Some(&png)));
        live.apply(&read_call("r2", "/srv/project/b.png"));
        live.apply(&read_done("r2", Some(&png)));
        let line = format!("[image: image-{}]", image_key(&png));
        assert_eq!(agent_lines(&live), [format!("{line}\n")]);
        // Nor again once the turn's files are read.
        let picture = Picture {
            mime: "image/png".into(),
            data: png.clone(),
        };
        live.show_files(None, vec![picture]);
        assert_eq!(agent_lines(&live).len(), 1);
        // The next turn may show as many as the cap.
        live.prompted("look at many");
        for n in 0..image_files::MAX_PER_TURN + 2 {
            let mut png = png.clone();
            png.push(u8::try_from(n).expect("a few"));
            let id = format!("m{n}");
            live.apply(&read_call(&id, &format!("/srv/project/{n}.png")));
            live.apply(&read_done(&id, Some(&png)));
        }
        assert_eq!(agent_lines(&live).len(), 1 + image_files::MAX_PER_TURN);
    }

    #[test]
    fn a_finished_edit_counts_its_file_relative_to_the_folder() {
        let mut live = live();
        assert!(live.apply(&update(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Edit src/main.rs",
            "kind": "edit",
            "status": "pending",
            "locations": [{"path": "/srv/project/src/main.rs"}]
        }))));
        assert_eq!(live.info.state.summary, "Edit src/main.rs");
        assert!(live.info.state.files_touched.is_empty());
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "done"}}]
        })));
        assert_eq!(live.info.state.files_touched, ["src/main.rs"]);
        let (entries, _) = live.transcript(1);
        assert_eq!(entries[0].text, "Edit src/main.rs [completed]\ndone");
    }

    #[test]
    fn a_plan_replaces_the_last() {
        let mut live = live();
        assert!(live.apply(&update(json!({
            "sessionUpdate": "plan",
            "entries": [
                {"content": "Read", "priority": "high", "status": "completed"},
                {"content": "Write", "priority": "high", "status": "in_progress"}
            ]
        }))));
        assert_eq!(live.info.state.plan[1].status, PlanStatus::InProgress);
    }

    #[test]
    fn a_permission_request_reads_as_a_question() {
        let live = live();
        let question = live.question(
            2,
            &permission(json!({
                "sessionId": "s",
                "toolCall": {"toolCallId": "t9", "title": "Run cargo test", "rawInput": {"command": "cargo test"}},
                "options": [
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "reject", "name": "Reject", "kind": "reject_once"}
                ]
            })),
        );
        assert_eq!(question.id, 2);
        assert_eq!(question.text, "Run cargo test");
        assert_eq!(question.detail.as_deref(), Some("cargo test"));
        assert_eq!(question.options[1].kind, OptionKind::RejectOnce);
        let titled_by_command = live.question(
            3,
            &permission(json!({"sessionId": "s", "toolCall": {"toolCallId": "t2", "title": "ls", "rawInput": {"command": "ls"}}, "options": []})),
        );
        assert_eq!(titled_by_command.detail, None);
    }

    #[test]
    fn the_transcript_drops_its_oldest_entries() {
        let mut live = live();
        for n in 0..MAX_ENTRIES + 3 {
            live.push(EntryKind::User, &n.to_string());
        }
        let (entries, earlier) = live.transcript(2);
        assert_eq!(earlier, MAX_ENTRIES as u64 + 1);
        assert_eq!(entries[1].text, (MAX_ENTRIES + 2).to_string());
    }

    #[tokio::test]
    async fn a_running_cost_is_counted_per_turn() {
        let usage = Arc::new(UsageHub::default());
        let mut live = Live::new(
            SessionInfo {
                id: "a3f9c1".into(),
                agent: "opencode".into(),
                cwd: "/srv/project".into(),
                ..SessionInfo::default()
            },
            Arc::default(),
            usage.clone(),
        );
        let cost = |amount: f64| {
            update(
                json!({"sessionUpdate": "usage_update", "used": 9, "size": 10,
                "cost": {"amount": amount, "currency": "USD"}}),
            )
        };
        live.apply(&cost(0.5));
        live.apply(&cost(1.25));
        live.spent(TurnTokens {
            total: 900,
            models: Vec::new(),
        });
        let first = usage.next_turn().await;
        assert_eq!(first.cost_usd, Some(1.25));
        assert_eq!((first.tokens, first.agent.as_str()), (900, "opencode"));
        live.apply(&cost(1.5));
        live.spent(TurnTokens::default());
        assert_eq!(usage.next_turn().await.cost_usd, Some(0.25));
        // A turn that spent nothing is not counted.
        live.spent(TurnTokens::default());
        live.spent(TurnTokens {
            total: 5,
            models: Vec::new(),
        });
        let last = usage.next_turn().await;
        assert_eq!((last.tokens, last.cost_usd), (5, None));
    }

    #[test]
    fn usage_commands_and_compactions_are_kept() {
        let mut live = live();
        assert!(!live.apply(&update(
            json!({"sessionUpdate": "usage_update", "used": 900, "size": 1000})
        )));
        assert_eq!(
            live.context,
            Some(Usage {
                used: 900,
                size: 1000
            })
        );
        live.apply(&update(
            json!({"sessionUpdate": "available_commands_update",
            "availableCommands": [{"name": "compact", "description": "Compact"}]}),
        ));
        assert_eq!(live.commands, ["compact"]);
        let compaction = |status: &str| {
            update(
                json!({"sessionUpdate": "compaction_update", "compactionId": "c1", "status": status}),
            )
        };
        assert!(live.apply(&compaction("in_progress")));
        live.apply(&compaction("completed"));
        live.apply(&compaction("completed"));
        let (entries, _) = live.transcript(10);
        assert_eq!(entries.len(), 1, "shown once");
        assert_eq!(entries[0].kind, EntryKind::Other);
        assert_eq!(entries[0].text, COMPACTED_ENTRY);
        assert_eq!(live.context, None, "the usage before no longer holds");
        assert!(live.take_compacted());
        assert!(!live.take_compacted());
    }

    #[test]
    fn cutting_keeps_characters_whole() {
        assert_eq!(cut("héllo", 2), "h");
        assert_eq!(cut("hi", 10), "hi");
    }

    #[test]
    fn a_thought_shows_its_heading_or_first_whole_sentence() {
        assert_eq!(
            thought_line("**Planning the fix**\n\nI will"),
            Some("Planning the fix".into())
        );
        assert_eq!(
            thought_line("**Reading**\n\nok. **Running the tests** now"),
            Some("Running the tests".into())
        );
        assert_eq!(
            thought_line("Let me check `src/main.rs`. Then"),
            Some("Let me check src/main.rs.".into())
        );
        assert_eq!(thought_line("Looking at e.g"), None, "not whole yet");
        assert_eq!(thought_line("**Half"), None);
        assert_eq!(
            thought_line("First line\nsecond"),
            Some("First line".into())
        );
    }

    #[test]
    fn a_subagents_calls_go_on_the_call_that_started_it() {
        let mut live = live();
        live.prompted("look around");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "task", "title": "Explore the code",
            "status": "in_progress", "_meta": {"claudeCode": {"toolName": "Agent"}}
        })));
        let inner = |id: &str, title: &str| {
            update(json!({
                "sessionUpdate": "tool_call", "toolCallId": id, "title": title, "status": "in_progress",
                "_meta": {"claudeCode": {"toolName": "Read", "parentToolUseId": "task"}}
            }))
        };
        live.apply(&inner("r1", "Read src/main.rs"));
        live.apply(&inner("r2", "grep -n todo"));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "r1", "status": "completed",
            "_meta": {"claudeCode": {"parentToolUseId": "task"}}
        })));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "task", "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "Found it"}}]
        })));
        let (entries, _) = live.transcript(10);
        assert_eq!(
            entries.len(),
            2,
            "the subagent's calls are not entries of their own"
        );
        assert_eq!(
            entries[1].text,
            "Explore the code [completed]\n[steps] 2\n[step completed] Read src/main.rs\n[step in_progress] grep -n todo\nFound it"
        );
    }

    #[test]
    fn a_call_whose_parent_is_unknown_is_its_own_entry() {
        let mut live = live();
        live.prompted("hi");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "r1", "title": "Read a.rs", "status": "completed",
            "_meta": {"claudeCode": {"parentToolUseId": "gone"}}
        })));
        assert_eq!(live.transcript(10).0[1].text, "Read a.rs [completed]");
    }

    #[test]
    fn a_claude_subagent_names_its_type_before_it_runs_a_step() {
        let mut live = live();
        live.prompted("check the release");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "task", "title": "Check the notes",
            "status": "in_progress",
            "rawInput": {"description": "Check the notes", "subagent_type": "release-checker",
                "prompt": "Compare the notes with the diff"}
        })));
        assert_eq!(
            live.transcript(10).0[1].text,
            "Check the notes [in_progress]\n[steps] 0\n[agent] release-checker"
        );
    }

    #[test]
    fn a_codex_subagent_shows_on_its_spawn_and_takes_its_rollouts_steps() {
        let mut live = live();
        live.prompted("spawn a subagent that sleeps");
        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "spawn", "title": "spawnAgent",
            "kind": "other", "status": "in_progress",
            "rawInput": {"prompt": "Sleep for 60 seconds", "senderThreadId": "root",
                "receiverThreadIds": [], "agentsStates": {}}
        })));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "spawn", "title": "spawnAgent",
            "status": "completed",
            "rawInput": {"prompt": "Sleep for 60 seconds", "senderThreadId": "root",
                "receiverThreadIds": ["t1"], "agentsStates": {"t1": {"status": "running"}}}
        })));
        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "act1", "title": "Start subagent sleep_test",
            "kind": "other", "status": "in_progress",
            "rawInput": {"agentThreadId": "t1", "agentPath": "/root/sleep_test",
                "activityKind": "started"}
        })));
        let (entries, _) = live.transcript(10);
        assert_eq!(entries.len(), 2, "the activity is not an entry of its own");
        assert_eq!(entries[1].text, "sleep_test [in_progress]\n[steps] 0");
        let children = live.codex_children(0);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].threads, ["t1"]);

        let run = Run {
            steps: vec![RunStep {
                title: "sleep".into(),
                done: true,
            }],
            count: 1,
            result: Some("Slept for a minute.".into()),
            working: false,
            name: Some("Kepler".into()),
            task: Some("sleep_test".into()),
        };
        let (at, text) = live.with_run(&children[0].call, &run).expect("its entry");
        assert_eq!(at, 1);
        assert_eq!(
            text,
            "sleep_test [completed]\n[steps] 1\n[step completed] sleep\n[agent] Kepler\nSlept for a minute."
        );

        live.apply(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "act2", "title": "Complete subagent sleep_test",
            "kind": "other", "status": "completed",
            "rawInput": {"agentThreadId": "t1", "agentPath": "/root/sleep_test",
                "activityKind": "completed"}
        })));
        assert_eq!(
            live.transcript(10).0[1].text,
            "sleep_test [completed]\n[steps] 0"
        );
    }
}
