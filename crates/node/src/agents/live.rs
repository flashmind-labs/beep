//! A session as the daemon keeps it: the compact state the server follows,
//! built from the agent's `session/update` notifications, and the
//! transcript, which stays here.

use beep_proto::node::agent::{
    AnswerOption, Entry, EntryKind, OptionKind, PlanEntry, PlanStatus, Question, SessionInfo,
    Status, StopReason,
};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
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
/// The longest summary or question detail.
const MAX_LINE: usize = 300;

/// ACP tool kinds that change files.
const EDIT_KINDS: &[&str] = &["edit", "delete", "move"];

#[derive(Debug, Default)]
pub(crate) struct Live {
    pub(crate) info: SessionInfo,
    /// Why the last turn ended.
    pub(crate) last_stop: Option<StopReason>,
    /// The agent's own id for the session, once it gave one: for Claude
    /// Code, the transcript's name.
    pub(crate) agent_session: Option<String>,
    transcript: VecDeque<Entry>,
    /// Entries dropped from the front of `transcript`.
    dropped: u64,
    /// The tool calls seen, by ACP id.
    tools: HashMap<String, ToolCall>,
    /// The last agent message is still arriving in chunks.
    in_message: bool,
}

#[derive(Debug)]
struct ToolCall {
    /// Its entry, counted from the start of the session.
    entry: u64,
    title: String,
    kind: String,
    status: String,
    paths: Vec<String>,
    output: String,
}

impl Live {
    pub(crate) fn new(info: SessionInfo) -> Self {
        Self {
            info,
            ..Self::default()
        }
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
    pub(crate) fn question(&self, id: u32, params: &Value) -> Question {
        let call = &params["toolCall"];
        let known = call["toolCallId"]
            .as_str()
            .and_then(|id| self.tools.get(id));
        let text = call["title"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| known.map(|tool| tool.title.clone()))
            .unwrap_or_else(|| "Use a tool".into());
        let options = params["options"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|option| {
                Some(AnswerOption {
                    id: option["optionId"].as_str()?.to_owned(),
                    name: option["name"].as_str().unwrap_or_default().to_owned(),
                    kind: OptionKind::from_acp(option["kind"].as_str().unwrap_or_default()),
                })
            })
            .collect();
        let text = cut(&text, MAX_LINE);
        // Some agents title the question with the command itself.
        let detail = detail(&call["rawInput"]).filter(|detail| *detail != text);
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
    pub(crate) fn apply(&mut self, update: &Value) -> bool {
        let before = self.info.state.clone();
        let title = self.info.title.clone();
        match update["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                if let Some(text) = text_of(&update["content"]) {
                    self.message_chunk(text);
                }
            }
            "agent_thought_chunk" => {
                if self.info.state.summary.is_empty() {
                    self.info.state.summary = "Thinking".into();
                }
            }
            "tool_call" => self.tool_call(update),
            "tool_call_update" => self.tool_call_update(update),
            "plan" => {
                self.info.state.plan = update["entries"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .take(MAX_PLAN)
                    .map(|entry| PlanEntry {
                        content: cut(entry["content"].as_str().unwrap_or_default(), MAX_LINE),
                        status: match entry["status"].as_str() {
                            Some("in_progress") => PlanStatus::InProgress,
                            Some("completed") => PlanStatus::Completed,
                            _ => PlanStatus::Pending,
                        },
                    })
                    .collect();
            }
            "session_info_update" => {
                if let Some(new) = update["title"].as_str() {
                    self.info.title = Some(cut(new, MAX_LINE));
                }
            }
            _ => {}
        }
        self.info.state != before || self.info.title != title
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

    fn tool_call(&mut self, update: &Value) {
        let Some(id) = update["toolCallId"].as_str() else {
            return;
        };
        self.in_message = false;
        let mut tool = ToolCall {
            entry: 0,
            title: update["title"].as_str().unwrap_or("Tool").to_owned(),
            kind: update["kind"].as_str().unwrap_or("other").to_owned(),
            status: update["status"].as_str().unwrap_or("pending").to_owned(),
            paths: paths(update),
            output: String::new(),
        };
        tool.entry = self.push_entry(EntryKind::Tool, tool.entry_text());
        self.info.state.summary = cut(&tool.title, MAX_LINE);
        self.finish_tool(&tool);
        self.tools.insert(id.to_owned(), tool);
    }

    fn tool_call_update(&mut self, update: &Value) {
        let Some(id) = update["toolCallId"].as_str() else {
            return;
        };
        let Some(mut call) = self.tools.remove(id) else {
            return;
        };
        if let Some(title) = update["title"].as_str() {
            call.title = title.to_owned();
            self.info.state.summary = cut(title, MAX_LINE);
        }
        if let Some(kind) = update["kind"].as_str() {
            call.kind = kind.to_owned();
        }
        if let Some(status) = update["status"].as_str() {
            call.status = status.to_owned();
        }
        let more = paths(update);
        if !more.is_empty() {
            call.paths = more;
        }
        if let Some(content) = update["content"].as_array() {
            call.output = content
                .iter()
                .filter_map(content_text)
                .collect::<Vec<_>>()
                .join("\n");
        }
        let text = call.entry_text();
        if let Some(entry) = self.entry_mut(call.entry) {
            entry.text = text;
        }
        self.finish_tool(&call);
        self.tools.insert(id.to_owned(), call);
    }

    /// Notes the files a finished edit changed.
    fn finish_tool(&mut self, tool: &ToolCall) {
        if tool.status != "completed" || !EDIT_KINDS.contains(&tool.kind.as_str()) {
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
        let mut text = format!("{} [{}]", self.title, self.status);
        if !self.output.is_empty() {
            text.push('\n');
            text.push_str(&self.output);
        }
        cut(&text, MAX_ENTRY)
    }
}

/// The text of an ACP content block, when it is text.
fn text_of(content: &Value) -> Option<&str> {
    (content["type"] == "text").then(|| content["text"].as_str())?
}

/// The text of one entry in a tool call's `content`.
fn content_text(item: &Value) -> Option<String> {
    match item["type"].as_str()? {
        "content" => text_of(&item["content"]).map(str::to_owned),
        "diff" => Some(format!(
            "(changed {})",
            item["path"].as_str().unwrap_or("a file")
        )),
        "terminal" => Some("(ran in a terminal)".into()),
        _ => None,
    }
}

/// The files a tool call names, in `locations` or its input.
fn paths(update: &Value) -> Vec<String> {
    let mut found: Vec<String> = update["locations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|location| location["path"].as_str().map(str::to_owned))
        .collect();
    if found.is_empty() {
        let input = &update["rawInput"];
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn live() -> Live {
        Live::new(SessionInfo {
            cwd: "/srv/project".into(),
            ..SessionInfo::default()
        })
    }

    #[test]
    fn message_chunks_join_into_one_entry() {
        let mut live = live();
        live.prompted("hi");
        let chunk = |text: &str| json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}});
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
    fn a_finished_edit_counts_its_file_relative_to_the_folder() {
        let mut live = live();
        assert!(live.apply(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Edit src/main.rs",
            "kind": "edit",
            "status": "pending",
            "locations": [{"path": "/srv/project/src/main.rs"}]
        })));
        assert_eq!(live.info.state.summary, "Edit src/main.rs");
        assert!(live.info.state.files_touched.is_empty());
        live.apply(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "done"}}]
        }));
        assert_eq!(live.info.state.files_touched, ["src/main.rs"]);
        let (entries, _) = live.transcript(1);
        assert_eq!(entries[0].text, "Edit src/main.rs [completed]\ndone");
    }

    #[test]
    fn a_plan_replaces_the_last() {
        let mut live = live();
        assert!(live.apply(&json!({
            "sessionUpdate": "plan",
            "entries": [
                {"content": "Read", "priority": "high", "status": "completed"},
                {"content": "Write", "priority": "high", "status": "in_progress"}
            ]
        })));
        assert_eq!(live.info.state.plan[1].status, PlanStatus::InProgress);
    }

    #[test]
    fn a_permission_request_reads_as_a_question() {
        let live = live();
        let question = live.question(
            2,
            &json!({
                "sessionId": "s",
                "toolCall": {"toolCallId": "t9", "title": "Run cargo test", "rawInput": {"command": "cargo test"}},
                "options": [
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "reject", "name": "Reject", "kind": "reject_once"}
                ]
            }),
        );
        assert_eq!(question.id, 2);
        assert_eq!(question.text, "Run cargo test");
        assert_eq!(question.detail.as_deref(), Some("cargo test"));
        assert_eq!(question.options[1].kind, OptionKind::RejectOnce);
        let titled_by_command = live.question(
            3,
            &json!({"toolCall": {"title": "ls", "rawInput": {"command": "ls"}}, "options": []}),
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
    fn cutting_keeps_characters_whole() {
        assert_eq!(cut("héllo", 2), "h");
        assert_eq!(cut("hi", 10), "hi");
    }
}
