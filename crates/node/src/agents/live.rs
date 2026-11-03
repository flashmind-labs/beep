//! A session as the daemon keeps it: the compact state the server follows,
//! built from the agent's `session/update` notifications, and the
//! transcript, which stays here.

use super::{
    acp::wire_name,
    compaction::{COMPACTED_ENTRY, Usage},
    images::{Images, Picture, data_url},
};
use agent_client_protocol::schema::{
    MaybeUndefined,
    v1::{
        CompactionId, CompactionStatus, CompactionUpdate, ContentBlock, PlanEntryStatus,
        RequestPermissionRequest, SessionUpdate, ToolCall as AcpToolCall, ToolCallContent,
        ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolKind,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::node::agent::{
    AnswerOption, Entry, EntryKind, OptionKind, PlanEntry, PlanStatus, Question, SessionInfo,
    Status, StopReason,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    sync::Arc,
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
    pub(crate) usage: Option<Usage>,
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
    /// The last agent message is still arriving in chunks.
    in_message: bool,
    /// Where the pictures its transcript shows are kept, shared by the
    /// node's sessions.
    images: Arc<Images>,
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
}

impl Live {
    pub(crate) fn new(info: SessionInfo, images: Arc<Images>) -> Self {
        Self {
            info,
            images,
            ..Self::default()
        }
    }

    /// Keeps a picture the transcript shows, for the server to ask for.
    pub(crate) fn keep(&self, picture: Picture) {
        self.images.keep(&self.info.id, picture);
    }

    /// The kept picture `key` of this session.
    #[cfg(test)]
    pub(crate) fn picture(&self, key: &str) -> Option<Arc<Picture>> {
        self.images.get(&self.info.id, key)
    }

    pub(crate) fn status(&self) -> Status {
        self.info.state.status
    }

    /// The last `last` entries and how many came before them.
    pub(crate) fn transcript(&self, last: usize) -> (Vec<Entry>, u64) {
        let skip = self.transcript.len().saturating_sub(last);
        let entries = self.transcript.iter().skip(skip).cloned().collect();
        (entries, self.dropped + skip as u64)
    }

    pub(crate) fn push(&mut self, kind: EntryKind, text: &str) {
        self.in_message = false;
        self.push_entry(kind, cut(text, MAX_ENTRY));
    }

    fn push_entry(&mut self, kind: EntryKind, text: String) -> u64 {
        if self.transcript.len() >= MAX_ENTRIES {
            self.transcript.pop_front();
            self.dropped += 1;
        }
        self.transcript.push_back(Entry { kind, text });
        self.dropped + self.transcript.len() as u64 - 1
    }

    fn entry_mut(&mut self, at: u64) -> Option<&mut Entry> {
        let index = usize::try_from(at.checked_sub(self.dropped)?).ok()?;
        self.transcript.get_mut(index)
    }

    /// A prompt went to the agent.
    pub(crate) fn prompted(&mut self, text: &str) {
        self.push(EntryKind::User, text);
        let state = &mut self.info.state;
        state.status = Status::Working;
        state.summary = "Thinking".into();
        state.error = None;
    }

    /// The question the agent asked in `session/request_permission`.
    pub(crate) fn question(&self, id: u32, request: &RequestPermissionRequest) -> Question {
        let call = &request.tool_call;
        let known = self.tools.get(&call.tool_call_id);
        let text = call
            .fields
            .title
            .clone()
            .or_else(|| known.map(|tool| tool.title.clone()))
            .unwrap_or_else(|| "Use a tool".into());
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
        let before = self.info.state.clone();
        let title = self.info.title.clone();
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    self.message_chunk(&text.text);
                }
            }
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
            SessionUpdate::AgentThoughtChunk(_) => {
                if self.info.state.summary.is_empty() {
                    self.info.state.summary = "Thinking".into();
                }
            }
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
                self.usage = Some(Usage {
                    used: usage.used,
                    size: usage.size,
                });
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
        self.usage = None;
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
        let mut tool = ToolCall {
            entry: 0,
            title: call.title.clone(),
            kind: call.kind,
            status: call.status,
            paths: paths(&call.locations, call.raw_input.as_ref()),
            output: output(&call.content),
        };
        tool.entry = self.push_entry(EntryKind::Tool, tool.entry_text());
        self.info.state.summary = cut(&tool.title, MAX_LINE);
        self.finish_tool(&tool);
        self.tools.insert(call.tool_call_id.clone(), tool);
    }

    fn tool_call_update(&mut self, update: &ToolCallUpdate) {
        let Some(mut call) = self.tools.remove(&update.tool_call_id) else {
            return;
        };
        let fields = &update.fields;
        if let Some(title) = &fields.title {
            call.title.clone_from(title);
            self.info.state.summary = cut(title, MAX_LINE);
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
        if let Some(content) = &fields.content {
            call.output = output(content);
        }
        let text = call.entry_text();
        if let Some(entry) = self.entry_mut(call.entry) {
            entry.text = text;
        }
        self.finish_tool(&call);
        self.tools.insert(update.tool_call_id.clone(), call);
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

impl ToolCall {
    fn entry_text(&self) -> String {
        let mut text = format!("{} [{}]", self.title, wire_name(&self.status));
        if !self.output.is_empty() {
            text.push('\n');
            text.push_str(&self.output);
        }
        cut(&text, MAX_ENTRY)
    }
}

/// The text a tool call's `content` shows, cut to an entry's length.
fn output(content: &[ToolCallContent]) -> String {
    let text = content
        .iter()
        .filter_map(|item| match item {
            ToolCallContent::Content(content) => match &content.content {
                ContentBlock::Text(text) => Some(text.text.clone()),
                _ => None,
            },
            ToolCallContent::Diff(diff) => Some(format!("(changed {})", diff.path.display())),
            ToolCallContent::Terminal(_) => Some("(ran in a terminal)".into()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    cut(&text, MAX_ENTRY)
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

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::image_key;
    use serde_json::json;

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
        )
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

    #[test]
    fn usage_commands_and_compactions_are_kept() {
        let mut live = live();
        assert!(!live.apply(&update(
            json!({"sessionUpdate": "usage_update", "used": 900, "size": 1000})
        )));
        assert_eq!(
            live.usage,
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
        assert_eq!(live.usage, None, "the usage before no longer holds");
        assert!(live.take_compacted());
        assert!(!live.take_compacted());
    }

    #[test]
    fn cutting_keeps_characters_whole() {
        assert_eq!(cut("héllo", 2), "h");
        assert_eq!(cut("hi", 10), "hi");
    }
}
