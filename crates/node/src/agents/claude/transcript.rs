//! Claude Code's transcripts, `<config>/projects/<folder slug>/<id>.jsonl`,
//! digested into prompts, text, thinking, tool calls and turn ends (`Digest`,
//! `digest_tail`, `head`).

use crate::agents::{
    compaction::{COMPACTED_ENTRY, summary_note},
    live::{ARTIFACT_TOOL, SKILL_TOOL},
    tool_details,
};
use crate::{
    agents::calls::Tool as BeepTool,
    agents::tags::{self, Part},
    agents::{
        Picture, block_picture, cut,
        found::unix_millis,
        window::{Excerpt, content_text, finish_tool_call, window},
    },
    fs::fsat::{self, FileId},
};
use beep_proto::{
    AgentToolCall,
    node::agent::{Entry, EntryKind},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime},
};

/// The first window of a file's tail that is read...
const FIRST_WINDOW: u64 = 1024 * 1024;
/// ...and the largest, when the first holds too few entries.
const MAX_WINDOW: u64 = 16 * 1024 * 1024;
/// How much of a file's head is read for when it started.
const HEAD: u64 = 64 * 1024;
/// The longest entry kept.
const MAX_ENTRY: usize = 4 * 1024;
/// The longest prompt or message in a summary.
pub(super) const MAX_SHORT: usize = 300;
/// The longest tool line.
const MAX_TOOL: usize = 160;

/// What a transcript says about a session as a whole.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Said {
    /// Where it works now: the folder of its newest tool line.
    pub(super) cwd: Option<String>,
    /// The folder it was started in, which Claude Code files it under and
    /// resumes it in, wherever it worked since.
    pub(super) launched: Option<String>,
    pub(super) title: Option<String>,
    /// A title the user set, which wins over the generated one.
    custom_title: Option<String>,
    pub(super) last_prompt: Option<String>,
    /// Claude Code's own record of the last prompt, for when the prompt
    /// itself is before the window.
    recorded_prompt: Option<String>,
    pub(super) last_message: Option<String>,
    /// Unix seconds of the last timestamped line.
    pub(super) last_at: Option<u64>,
}

impl Said {
    /// The title to show: the user's, else the generated one.
    pub(super) fn title(&self) -> Option<String> {
        self.custom_title
            .clone()
            .or_else(|| self.title.as_deref().and_then(tags::title))
    }

    pub(super) fn prompt(&self) -> Option<&str> {
        self.last_prompt
            .as_deref()
            .or(self.recorded_prompt.as_deref())
    }
}

/// What a stretch of transcript says.
#[derive(Debug, Default)]
pub(super) struct Digest {
    /// Native transcript files contain sidechain messages as their own conversation.
    native: bool,
    pub(super) said: Said,
    /// Unix seconds of the first timestamped line.
    pub(super) first_at: Option<u64>,
    /// Unix milliseconds of the line before the one being read, from when
    /// a thought that follows it started.
    before_ms: Option<u64>,
    /// Unix milliseconds of the line being read.
    line_ms: Option<u64>,
    pub(super) entries: Vec<Entry>,
    /// The entry of each tool call, by its id, to mark the failed ones.
    tools: HashMap<String, usize>,
    /// The subagents tool calls started, by the call's id.
    pub(super) spawns: HashMap<String, Spawn>,
    /// The ids of `Artifact` calls, whose entry gains the page's link.
    artifacts: HashSet<String>,
    /// Turns that ended in this stretch.
    pub(super) turns_ended: u32,
    /// The pictures the user's prompts held, each shown as its image line,
    /// when asked for; without, prompts show no images.
    pub(super) pictures: Option<Vec<Picture>>,
}

/// A tool call that started a subagent (Claude Code's `Agent`, `Task`
/// before it), as the session's own file has it.
#[derive(Debug, Default, Clone)]
pub(super) struct Spawn {
    /// Its entry among the digest's.
    pub(super) entry: usize,
    /// What it was asked to do, in a few words.
    pub(super) description: Option<String>,
    /// Its `subagent_type`: `Explore`, or a custom agent's name.
    pub(super) agent: Option<String>,
    pub(super) model: Option<String>,
    /// The id the subagent's file is named by, once its result said it.
    pub(super) agent_id: Option<String>,
    pub(super) failed: bool,
}

/// A page of entries and the subagent calls among them, which `subagents`
/// writes on their entries.
pub(super) struct Page {
    pub(super) excerpt: Excerpt,
    /// By tool call id.
    pub(super) spawns: HashMap<String, Spawn>,
    /// The digest's entry the page starts at.
    pub(super) skip: usize,
}

/// Where a published artifact's link starts.
const ARTIFACT_LINK: &str = "https://claude.ai/";

/// Whether `session` is a subagent's own transcript, kept in a `subagents`
/// folder.
pub(super) fn is_native(session: &Path) -> bool {
    session
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "subagents")
}

impl Digest {
    fn for_file(path: &Path, text: &str, pictures: bool) -> Self {
        let mut digest = Self {
            native: is_native(path),
            pictures: pictures.then(Vec::new),
            ..Self::default()
        };
        digest.lines(text);
        digest
    }
    /// Reads every complete line of `text`.
    pub(super) fn read(text: &str) -> Self {
        let mut digest = Self::default();
        digest.lines(text);
        digest
    }

    /// Reads every complete line of `text`, keeping prompts' pictures.
    #[cfg(test)]
    pub(super) fn with_pictures(text: &str) -> Self {
        let mut digest = Self {
            pictures: Some(Vec::new()),
            ..Self::default()
        };
        digest.lines(text);
        digest
    }

    pub(super) fn lines(&mut self, text: &str) {
        for line in text.lines() {
            self.line(line);
        }
    }

    pub(super) fn line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return;
        };
        // A sub-agent's lines belong to its own conversation.
        if !self.native && value["isSidechain"].as_bool() == Some(true) {
            return;
        }

        let at_ms = value["timestamp"].as_str().and_then(unix_millis);

        if let Some(at_ms) = at_ms {
            let at = at_ms / 1000;
            self.first_at.get_or_insert(at);
            self.said.last_at = Some(at);
        }

        self.before_ms = self.line_ms.take().or(self.before_ms);
        self.line_ms = at_ms;
        // A resumed Claude starts in the session's launch folder, so only a
        // tool line moves the folder: a restart alone must not leave the
        // worktree the session works in.
        if let Some(cwd) = value["cwd"].as_str() {
            self.said.launched.get_or_insert_with(|| cwd.to_owned());
            if self.said.cwd.is_none() || runs_tool(&value) {
                self.said.cwd = Some(cwd.to_owned());
            }
        }
        match value["type"].as_str().unwrap_or("") {
            "user" => self.user(&value),
            "assistant" => self.assistant(&value),
            "system" => match value["subtype"].as_str() {
                Some("turn_duration") => self.turns_ended += 1,
                Some("compact_boundary") => self.push(EntryKind::Compacted, COMPACTED_ENTRY),
                _ => {}
            },
            "ai-title" => self.said.title = text_field(&value, "aiTitle"),
            "custom-title" => self.said.custom_title = text_field(&value, "customTitle"),
            "summary" => self.said.title = self.said.title.take().or(text_field(&value, "summary")),
            "last-prompt" => self.said.recorded_prompt = text_field(&value, "lastPrompt"),
            _ => {}
        }
    }

    fn user(&mut self, value: &Value) {
        if value["isCompactSummary"] == true {
            if let Some(summary) = content_text(&value["message"]["content"]) {
                if !self.entries.last().is_some_and(|entry| {
                    entry.kind == EntryKind::Compacted && entry.text == COMPACTED_ENTRY
                }) {
                    self.push(EntryKind::Compacted, COMPACTED_ENTRY);
                }
                if let Some(entry) = self.entries.last_mut() {
                    entry.text = summary_note(&summary);
                }
            }
            return;
        }
        if value["isMeta"] == true {
            return;
        }
        // Prompts other than the user's own, such as a finished task's notice.
        if !self.native
            && let Some(kind) = value["origin"]["kind"].as_str()
            && kind != "human"
        {
            return;
        }

        let content = &value["message"]["content"];
        let mut lines = String::new();
        let mut text = match content {
            Value::String(text) => Some(tags::untagged(text)),
            Value::Array(blocks) => {
                for block in blocks.iter().filter(|block| block["type"] == "tool_result") {
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    if self.native {
                        self.native_tool_result(id, block);
                    } else if block["is_error"].as_bool() == Some(true) {
                        self.tool_failed(id);
                    } else if self.artifacts.remove(id) {
                        self.artifact_published(id, &block["content"]);
                    }

                    if let Some(spawn) = self.spawns.get_mut(id)
                        && let Some(agent_id) = value["toolUseResult"]["agentId"].as_str()
                    {
                        spawn.agent_id = Some(agent_id.to_owned());
                    }

                    self.forked_skill(id, &value["toolUseResult"]);

                    if let Some(entry) = self.tool_entry(id)
                        && let Some(tool) = &mut entry.tool
                    {
                        (tool.result, tool.result_truncated) =
                            tool_details::json(block.get("content"));
                    }

                    self.tool_pictures(id, &block["content"]);
                }

                if let Some(pictures) = &mut self.pictures {
                    for picture in blocks.iter().filter_map(block_picture) {
                        lines.push('\n');
                        lines.push_str(&picture.line());
                        pictures.push(picture);
                    }
                }

                let texts: Vec<String> = blocks
                    .iter()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .map(tags::bare_block)
                    .collect();
                (!texts.is_empty()).then(|| texts.join("\n"))
            }
            _ => None,
        };
        // Claude Code writes a cut-off turn as a user line, but nobody said it.
        if let Some(rest) = text.as_deref().and_then(tags::after_interruption) {
            let rest = rest.to_owned();
            self.push(EntryKind::Other, "Interrupted");
            if rest.trim().is_empty() {
                return;
            }
            text = Some(rest);
        }
        // Beep's context and what other sessions sent stay apart from the
        // user's words, which Claude Code's own tags may wrap.
        let parts = text.as_deref().map(tags::parts).unwrap_or_default();
        let user: Vec<&str> = parts
            .iter()
            .filter_map(|part| match part {
                Part::User(text) => Some(*text),
                _ => None,
            })
            .collect();
        let prompt = prompt_text(&user.join("\n"));
        let others: Vec<Part> = parts
            .into_iter()
            .filter(|part| !matches!(part, Part::User(_)))
            .collect();
        let said = prompt.as_deref().or_else(|| {
            others.iter().find_map(|part| match part {
                Part::Session { text, .. } | Part::Child { text, .. } => Some(*text),
                _ => None,
            })
        });
        if let Some(said) = said {
            self.said.last_prompt = Some(cut(said.trim(), MAX_SHORT));
        } else if lines.is_empty() {
            return;
        }
        // The image lines go whole after the text, cut to make room.
        let room = MAX_ENTRY.saturating_sub(lines.len());
        let mut kept = others;
        if let Some(prompt) = &prompt {
            kept.push(Part::User(prompt));
        }

        let mut shown = cut(tags::join(&kept).trim(), room);
        shown.push_str(&lines);
        self.push(EntryKind::User, shown.trim_start());
    }

    fn assistant(&mut self, value: &Value) {
        let Value::Array(blocks) = &value["message"]["content"] else {
            return;
        };

        for block in blocks {
            match block["type"].as_str() {
                Some("text") => {
                    let text = block["text"].as_str().unwrap_or("").trim();

                    if text.is_empty() {
                        continue;
                    }

                    self.said.last_message = Some(cut(text, MAX_SHORT));

                    match self.entries.last_mut() {
                        Some(last) if last.kind == EntryKind::Agent => {
                            let joined = format!("{}\n\n{text}", last.text);
                            last.text = cut(&joined, MAX_ENTRY);
                        }
                        _ => self.push(EntryKind::Agent, text),
                    }
                }
                Some("thinking") => self.thinking(block["thinking"].as_str().unwrap_or("")),
                Some("tool_use") => {
                    let id = block["id"].as_str();
                    // A subagent's call written again is the same call.
                    if id.is_some_and(|id| self.spawns.contains_key(id)) {
                        continue;
                    }

                    let line = if self.native {
                        let name = block["name"].as_str().unwrap_or("tool");
                        // A Beep tool reads by its own title, as `Run on <machine>: <command>`.
                        let detail = ["command", "file_path", "pattern", "path"]
                            .iter()
                            .find_map(|key| block["input"][key].as_str())
                            .filter(|_| BeepTool::of_name(name).is_none());
                        format!(
                            "{} [in_progress]",
                            detail.map_or_else(
                                || tool_line(block),
                                |detail| format!("{name}: {detail}")
                            )
                        )
                    } else {
                        tool_line(block)
                    };
                    self.push(EntryKind::Tool, &line);
                    let entry = self.entries.len() - 1;
                    let (input, input_truncated) = tool_details::json(block.get("input"));
                    self.entries[entry].tool = Some(AgentToolCall {
                        name: text_field(block, "name"),
                        input,
                        input_truncated,
                        ..AgentToolCall::default()
                    });
                    let Some(id) = id else {
                        continue;
                    };
                    self.tools.insert(id.to_owned(), entry);
                    if block["name"].as_str() == Some(ARTIFACT_TOOL) {
                        self.artifacts.insert(id.to_owned());
                    }
                    if block["name"]
                        .as_str()
                        .is_some_and(|name| crate::agents::subagent::CLAUDE_SPAWNS.contains(&name))
                    {
                        let input = &block["input"];
                        self.spawns.insert(
                            id.to_owned(),
                            Spawn {
                                entry,
                                description: text_field(input, "description"),
                                agent: text_field(input, "subagent_type"),
                                model: text_field(input, "model"),
                                ..Spawn::default()
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// A thinking block, as a thought entry that says how long it took:
    /// from the line before it to its own. Claude Code writes each block
    /// once it is whole, so its line is when the thought ended. A block
    /// kept without its text (thinking display `omitted`) shows nothing,
    /// as a live session's does; thoughts in a row read as one.
    fn thinking(&mut self, text: &str) {
        let text = text.trim();

        if text.is_empty() {
            return;
        }

        let took = match (self.before_ms, self.line_ms) {
            (Some(start), Some(end)) => end.checked_sub(start),
            _ => None,
        };

        match self.entries.last_mut() {
            Some(last) if last.kind == EntryKind::Thought => {
                let joined = format!("{}\n\n{text}", last.text);
                last.text = cut(&joined, MAX_ENTRY);
                last.ms = match (last.ms, took) {
                    (Some(before), Some(took)) => Some(before.saturating_add(took)),
                    _ => None,
                };
            }
            _ => {
                self.push(EntryKind::Thought, text);
                if let Some(entry) = self.entries.last_mut() {
                    entry.ms = took;
                }
            }
        }
    }

    /// The entry of the tool call `id`, when it is among the entries.
    fn tool_entry(&mut self, id: &str) -> Option<&mut Entry> {
        self.tools.get(id).and_then(|&at| self.entries.get_mut(at))
    }

    fn native_tool_result(&mut self, id: &str, block: &Value) {
        let failed = block["is_error"].as_bool() == Some(true);
        if failed && let Some(spawn) = self.spawns.get_mut(id) {
            spawn.failed = true;
        }

        let Some(entry) = self.tool_entry(id) else {
            return;
        };
        let output = content_text(&block["content"]).unwrap_or_default();
        finish_tool_call(&mut entry.text, failed, &output, MAX_ENTRY);
    }

    /// Retitles the `Skill` call `id` whose result says its skill forked:
    /// it ran as a subagent, not as a step of the turn.
    fn forked_skill(&mut self, id: &str, result: &Value) {
        if result["status"].as_str() != Some("forked") {
            return;
        }

        let Some(skill) = result["commandName"]
            .as_str()
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        let Some(entry) = self.tool_entry(id) else {
            return;
        };

        if let Some(rest) = entry.text.strip_prefix(SKILL_TOOL) {
            entry.text = cut(
                &format!("{SKILL_TOOL}: {} (subagent){rest}", cut(skill, MAX_SHORT)),
                MAX_ENTRY,
            );
        }
    }

    fn tool_failed(&mut self, id: &str) {
        if let Some(spawn) = self.spawns.get_mut(id) {
            spawn.failed = true;
        }
        if let Some(entry) = self.tool_entry(id) {
            entry.text.push_str(" (failed)");
        }
    }

    /// Writes the link an `Artifact` call's result gives after its entry,
    /// as a live session writes a finished call: `Artifact: <path>
    /// [completed]`, then the link.
    fn artifact_published(&mut self, id: &str, content: &Value) {
        let Some(text) = content_text(content) else {
            return;
        };
        let Some(link) = artifact_link(&text) else {
            return;
        };

        if let Some(entry) = self.tool_entry(id) {
            entry.text = format!("{} [completed]\n{link}", entry.text);
        }
    }

    /// Shows the pictures a tool's result held as image lines after the
    /// tool's entry, when pictures are read.
    fn tool_pictures(&mut self, id: &str, content: &Value) {
        let (Some(pictures), Value::Array(blocks)) = (&mut self.pictures, content) else {
            return;
        };
        let Some(entry) = self
            .tools
            .get(id)
            .and_then(|&index| self.entries.get_mut(index))
        else {
            return;
        };

        for picture in blocks.iter().filter_map(block_picture) {
            entry.text.push('\n');
            entry.text.push_str(&picture.line());
            pictures.push(picture);
        }
    }

    fn push(&mut self, kind: EntryKind, text: &str) {
        self.entries.push(Entry::new(kind, cut(text, MAX_ENTRY)));
    }
}

/// Whether a transcript line calls a tool or carries a tool's result.
fn runs_tool(value: &Value) -> bool {
    value["message"]["content"].as_array().is_some_and(|blocks| {
        blocks
            .iter()
            .any(|block| matches!(block["type"].as_str(), Some("tool_use" | "tool_result")))
    })
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| cut(text, MAX_SHORT))
}

/// What the user typed, from a user line's text. Claude Code wraps slash
/// commands and command output in tags; a command reads as `/name`, and
/// other wrapped text is not a prompt.
fn prompt_text(text: &str) -> Option<String> {
    let text = text.trim();

    if let Some(start) = text.find("<command-name>") {
        let rest = &text[start + "<command-name>".len()..];
        let name = rest.split("</command-name>").next()?.trim();
        let args = text
            .split("<command-args>")
            .nth(1)
            .and_then(|rest| rest.split("</command-args>").next())
            .map(str::trim)
            .unwrap_or("");
        let command = if name.starts_with('/') {
            name.to_owned()
        } else {
            format!("/{name}")
        };
        return Some(if args.is_empty() {
            command
        } else {
            format!("{command} {args}")
        });
    }

    if text.is_empty() || text.starts_with('<') {
        return None;
    }

    Some(text.to_owned())
}

/// The first claude.ai artifact link in `text`.
fn artifact_link(text: &str) -> Option<&str> {
    let start = text.find(ARTIFACT_LINK)?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '<' | '>' | ']' | '`'))
        .unwrap_or(rest.len());
    let link = rest[..end].trim_end_matches(['.', ',', ';']);
    link.contains("/artifact/").then_some(link)
}

/// A tool call as one line, such as `Bash: cargo test`.
pub(super) fn tool_line(block: &Value) -> String {
    let name = block["name"].as_str().unwrap_or("tool");
    let input = &block["input"];

    if let Some(tool) = BeepTool::of_name(name) {
        return cut(&tool.title(Some(input)), MAX_TOOL);
    }

    let detail = [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
        "prompt",
    ]
    .iter()
    .find_map(|key| input[key].as_str())
    .map(|detail| detail.lines().next().unwrap_or("").trim().to_owned());

    match detail {
        Some(detail) if !detail.is_empty() => cut(&format!("{name}: {detail}"), MAX_TOOL),
        _ => name.to_owned(),
    }
}

/// The complete lines at the end of a file.
pub(super) struct Tail {
    pub(super) text: String,
    /// The text starts at the start of the file.
    pub(super) whole: bool,
    /// Where the text ends in the file: after its last newline.
    pub(super) end: u64,
}

/// Reads the complete lines in the last `max` bytes of `path`. A long
/// session's file runs to tens of megabytes, mostly request records, so
/// only its tail is read.
pub(super) fn read_tail(path: &Path, max: u64) -> io::Result<Tail> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max);
    read_from(&mut file, start, len)
}

/// Reads the complete lines from `start` to `end`: a line `start` cuts is
/// dropped, and so is a last line not yet ended.
pub(super) fn read_from(file: &mut File, start: u64, end: u64) -> io::Result<Tail> {
    // One byte before `start` says whether it begins a line.
    let from = start.saturating_sub(1);
    file.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    file.take(end.saturating_sub(from))
        .read_to_end(&mut bytes)?;
    let mut first = 0;

    if start > 0 {
        first = match bytes.iter().position(|&b| b == b'\n') {
            Some(at) => at + 1,
            None => bytes.len(),
        };
    }

    let last = bytes[first..]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(first, |at| first + at + 1);
    let text = String::from_utf8_lossy(&bytes[first..last]).into_owned();
    Ok(Tail {
        text,
        whole: start == 0,
        end: from + last as u64,
    })
}

/// The digest of the end of a file, reading back far enough for `entries`
/// entries when it can.
pub(super) fn digest_tail(path: &Path, entries: usize) -> io::Result<Digest> {
    digest_tail_with(path, entries, |text| Digest::for_file(path, text, false))
        .map(|(digest, _)| digest)
}

/// As `digest_tail`, with where the text read ends in the file.
fn digest_tail_with(
    path: &Path,
    entries: usize,
    read: impl Fn(&str) -> Digest,
) -> io::Result<(Digest, u64)> {
    let mut window = FIRST_WINDOW;

    loop {
        let tail = read_tail(path, window)?;
        let digest = read(&tail.text);
        let enough = digest.entries.len() > entries && digest.said.prompt().is_some();

        if tail.whole || enough || window >= MAX_WINDOW {
            return Ok((digest, tail.end));
        }
        window *= 4;
    }
}

/// What a listing shows of a transcript, read from its tail and brought up
/// to date by parsing only what is appended after it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Gist {
    native: bool,
    pub(super) said: Said,
    /// Where the tail was read whole, and where the lines read end.
    read_at: u64,
    end: u64,
    /// The `CHECKED` bytes before `end`, to tell an append from a rewrite.
    check: Vec<u8>,
}

impl Gist {
    /// The tail of the transcript at `path`, as `digest_tail` reads it.
    pub(super) fn read(path: &Path) -> io::Result<Self> {
        let (digest, end) = digest_tail_with(path, 0, |text| Digest::for_file(path, text, false))?;
        let check = bytes_before(&mut File::open(path)?, end)?;
        Ok(Self::of(digest, end, end, check))
    }

    /// This gist after the lines appended to `path` since, or `None` when
    /// the file was written again or has grown by more than a tail read
    /// since its tail was read whole.
    pub(super) fn appended(&self, path: &Path) -> io::Result<Option<Self>> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();

        if len < self.end
            || len - self.read_at > FIRST_WINDOW
            || bytes_before(&mut file, self.end)? != self.check
        {
            return Ok(None);
        }

        let tail = read_from(&mut file, self.end, len)?;
        let mut digest = Digest {
            native: self.native,
            said: self.said.clone(),
            ..Digest::default()
        };
        digest.lines(&tail.text);
        let check = bytes_before(&mut file, tail.end)?;
        Ok(Some(Self::of(digest, self.read_at, tail.end, check)))
    }

    fn of(digest: Digest, read_at: u64, end: u64, check: Vec<u8>) -> Self {
        Self {
            native: digest.native,
            said: digest.said,
            read_at,
            end,
            check,
        }
    }
}

/// What the file's head says: where the session started, and when.
pub(super) fn head(path: &Path) -> io::Result<Digest> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let head = read_from(&mut file, 0, len.min(HEAD))?;
    Ok(Digest::for_file(path, &head.text, false))
}

/// Recover history omitted by ACP without duplicating the replay. Prefer
/// the longest match, then the newest one when conversations repeat.
pub(super) fn prefix_len(entries: &[Entry], replay: &[Entry]) -> usize {
    let anchors: Vec<_> = replay.iter().filter_map(anchor).collect();
    let native: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(at, entry)| Some((at, anchor(entry)?)))
        .collect();
    if anchors.is_empty() && replay.iter().all(|entry| entry.kind == EntryKind::Other) {
        entries.len()
    } else {
        native
            .iter()
            .enumerate()
            .filter_map(|(at, (index, _))| {
                let matched = native[at..]
                    .iter()
                    .map(|(_, anchor)| anchor)
                    .zip(&anchors)
                    .take_while(|(native, replay)| native.matches(replay))
                    .count();
                (matched > 0).then_some((matched, *index))
            })
            .max_by_key(|(matched, _)| *matched)
            .map_or(0, |(_, at)| at)
    }
}

#[derive(PartialEq, Eq)]
enum Anchor {
    Text {
        kind: EntryKind,
        text: String,
        truncated: bool,
    },
    Tool(Option<String>, Option<String>),
    Compacted(String),
}

impl Anchor {
    fn matches(&self, replay: &Self) -> bool {
        match (self, replay) {
            (
                Self::Text {
                    kind,
                    text,
                    truncated,
                },
                Self::Text {
                    kind: other_kind,
                    text: other,
                    ..
                },
            ) => kind == other_kind && (text == other || (*truncated && other.starts_with(text))),
            (Self::Compacted(left), Self::Compacted(right)) => {
                left.is_empty()
                    || right.is_empty()
                    || left.starts_with(right)
                    || right.starts_with(left)
            }
            _ => self == replay,
        }
    }
}

fn anchor(entry: &Entry) -> Option<Anchor> {
    match entry.kind {
        EntryKind::User if entry.text.starts_with("<beep-context>")
            && tags::parts(&entry.text).iter().all(|part| matches!(part, Part::Context(_))) => None,
        EntryKind::User | EntryKind::Agent | EntryKind::Thought => {
            // ACP streams blocks together; the native reader separates them.
            let text = cut(entry.text.trim(), MAX_ENTRY)
                .split_whitespace()
                .collect();
            Some(Anchor::Text {
                kind: entry.kind,
                text,
                truncated: entry.text.len() >= MAX_ENTRY,
            })
        }
        EntryKind::Tool => entry
            .tool
            .as_ref()
            .map(|tool| Anchor::Tool(tool.name.clone(), tool.input.clone())),
        EntryKind::Compacted => Some(Anchor::Compacted(
            entry
                .text
                .split_once('\n')
                .map_or("", |(_, summary)| summary)
                .to_owned(),
        )),
        _ => None,
    }
}

/// The `last` entries before entry `before` (the end when `None`) of the
/// transcript at `path`, with the pictures their image lines show. The
/// file is indexed from its first line and kept in `kept`, which
/// parses only the lines appended since, so every page counts entries from
/// the same first one.
pub(super) fn entries(
    path: &Path,
    last: usize,
    before: Option<u64>,
    kept: &Transcripts,
) -> io::Result<Page> {
    kept.with(path, |digest| {
        page(digest, window(0, digest.entries.len(), last, before))
    })
}

/// The newest `last` entries of the transcript at `path`, as `entries`
/// reads them, but none before entry `from` when the transcript reaches
/// it: a reader holding the entries from `from` on needs no earlier ones.
pub(super) fn newest_from(
    path: &Path,
    last: usize,
    from: u64,
    kept: &Transcripts,
) -> io::Result<Page> {
    kept.with(path, |digest| {
        let mut range = window(0, digest.entries.len(), last, None);

        if let Ok(from) = usize::try_from(from)
            && from <= range.end
        {
            range.start = range.start.max(from);
        }

        page(digest, range)
    })
}

/// The entries of `digest` in `range`, with their pictures and the calls
/// among them that started subagents.
fn page(digest: &Digest, range: Range<usize>) -> Page {
    let skip = range.start;
    let shown = range.clone();
    let entries = digest.entries[range].to_vec();
    let spawns = digest
        .spawns
        .iter()
        .filter(|(_, spawn)| shown.contains(&spawn.entry))
        .map(|(call, spawn)| (call.clone(), spawn.clone()))
        .collect();

    Page {
        excerpt: Excerpt {
            entries,
            earlier: skip as u64,
            pictures: digest.pictures.clone().unwrap_or_default(),
        },
        spawns,
        skip,
    }
}

/// How many bytes before a parsed transcript's end are compared on the next
/// read, to tell an append from a file written again.
const CHECKED: u64 = 256;

/// Parsed transcripts by path, at most `cap`. Each is kept while its file
/// only grows: an append parses just the new lines, carrying the tool,
/// subagent and picture state of the lines before, so it reads as a whole
/// parse of the same text would. A replacement or rewrite rebuilds the index.
#[derive(Debug)]
pub(crate) struct Transcripts {
    cap: usize,
    kept: Mutex<Slots>,
}

#[derive(Debug, Default)]
struct Slots {
    /// Counts lookups, to drop the slot used longest ago.
    clock: u64,
    slots: HashMap<PathBuf, (u64, Slot)>,
}

/// One file's parse, locked while it is brought up to date and read.
type Slot = Arc<Mutex<Option<Parsed>>>;

/// One transcript's parse and where in the file it stands.
#[derive(Debug)]
struct Parsed {
    digest: Digest,
    /// Offset after the last complete line.
    end: u64,
    /// The file's length and time when last read, to skip an unchanged one.
    len: u64,
    modified: SystemTime,
    /// Which file it was: one written to a new file and moved over the old
    /// is parsed again, whatever its length, time and bytes.
    id: FileId,
    /// The `CHECKED` bytes before `end`.
    check: Vec<u8>,
}

impl Transcripts {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            cap,
            kept: Mutex::new(Slots::default()),
        }
    }

    /// `read` of the transcript at `path`, brought up to date first. Readers
    /// of one file take turns; other files are read alongside.
    pub(super) fn with<T>(&self, path: &Path, read: impl FnOnce(&Digest) -> T) -> io::Result<T> {
        let slot = self.slot(path);
        let mut parsed = crate::lock(&slot);
        let result = refresh(path, &mut parsed);

        if result.is_err() {
            *parsed = None;
        }

        result?;
        let digest = &parsed.as_ref().ok_or(io::ErrorKind::NotFound)?.digest;
        Ok(read(digest))
    }

    fn slot(&self, path: &Path) -> Slot {
        let mut kept = crate::lock(&self.kept);
        kept.clock += 1;
        let now = kept.clock;

        if let Some((used, slot)) = kept.slots.get_mut(path) {
            *used = now;
            return slot.clone();
        }

        if kept.slots.len() >= self.cap
            && let Some(oldest) = kept
                .slots
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(path, _)| path.clone())
        {
            kept.slots.remove(&oldest);
        }

        let slot = Arc::new(Mutex::new(None));
        kept.slots.insert(path.to_owned(), (now, slot.clone()));
        slot
    }
}

/// Brings `parsed` up to the end of the file at `path`.
fn refresh(path: &Path, parsed: &mut Option<Parsed>) -> io::Result<()> {
    let started = Instant::now();
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    let (len, modified) = (meta.len(), meta.modified()?);
    let id = fsat::id_of_file(&file)?;

    if let Some(kept) = parsed.as_mut()
        && kept.id == id
    {
        if kept.len == len && kept.modified == modified {
            return Ok(());
        }

        if len >= kept.end && bytes_before(&mut file, kept.end)? == kept.check {
            kept.end = read_lines(&mut file, kept.end, len, &mut kept.digest)?;
            kept.check = bytes_before(&mut file, kept.end)?;
            (kept.len, kept.modified) = (len, modified);
            return Ok(());
        }
    }

    let mut digest = Digest::for_file(path, "", true);
    let end = read_lines(&mut file, 0, len, &mut digest)?;
    crate::agents::transcript_stage("parse", started);
    *parsed = Some(Parsed {
        digest,
        end,
        len,
        modified,
        id,
        check: bytes_before(&mut file, end)?,
    });
    Ok(())
}

/// Index complete JSONL records without buffering the entire file. An unfinished
/// final record is retried from its first byte on the next refresh.
fn read_lines(file: &mut File, start: u64, len: u64, digest: &mut Digest) -> io::Result<u64> {
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file.take(len - start));
    let mut line = Vec::new();
    let mut end = start;
    loop {
        line.clear();
        let count = reader.read_until(b'\n', &mut line)?;
        if line.last() != Some(&b'\n') {
            break;
        }
        if let Ok(record) = std::str::from_utf8(&line) {
            digest.line(record);
            // Keep image keys in the scrollback. Images::find_in_file reads the
            // bytes on demand, including pictures near the start of long chats.
            if let Some(pictures) = &mut digest.pictures {
                pictures.clear();
            }
        }
        end += count as u64;
    }
    Ok(end)
}

/// The up to `CHECKED` bytes before `end`.
fn bytes_before(file: &mut File, end: u64) -> io::Result<Vec<u8>> {
    let from = end.saturating_sub(CHECKED);
    file.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    file.take(end - from).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::{fs, io::Write};

    #[test]
    fn a_forked_skill_reads_as_a_subagent() {
        let mut digest = Digest::default();
        for (id, skill, result) in [
            (
                "inline",
                "demo",
                serde_json::json!({"success": true, "commandName": "demo"}),
            ),
            (
                "fork",
                "review",
                serde_json::json!({"success": true, "commandName": "review", "status": "forked", "agentId": "a1"}),
            ),
        ] {
            for line in [
                serde_json::json!({"type": "assistant", "message": {"content": [
                    {"type": "tool_use", "id": id, "name": "Skill", "input": {"skill": skill}}
                ]}}),
                serde_json::json!({"type": "user", "toolUseResult": result, "message": {"content": [
                    {"type": "tool_result", "tool_use_id": id, "content": "ok"}
                ]}}),
            ] {
                digest.line(&line.to_string());
            }
        }
        let titles: Vec<&str> = digest.entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(titles, ["Skill", "Skill: review (subagent)"]);
    }

    #[test]
    fn historical_calls_keep_arguments_and_results() {
        let mut digest = Digest::default();
        for line in [
            serde_json::json!({"type": "assistant", "timestamp": "2026-10-29T12:34:00Z", "message": {"content": [
                {"type": "tool_use", "id": "wait", "name": "Bash", "input": {"command": "sleep 45"}}
            ]}}),
            serde_json::json!({"type": "user", "timestamp": "2026-10-29T12:34:45Z", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "wait", "content": "Finished waiting"}
            ]}}),
        ] {
            digest.line(&line.to_string());
        }
        let tool = digest.entries[0].tool.as_ref().unwrap();
        assert_eq!(tool.name.as_deref(), Some("Bash"));
        assert_eq!(tool.input.as_deref(), Some(r#"{"command":"sleep 45"}"#));
        assert_eq!(tool.result.as_deref(), Some(r#""Finished waiting""#));
    }

    /// A fabricated transcript in Claude Code's format.
    pub(crate) const FIXTURE: &str = include_str!("fixtures/session.jsonl");

    #[test]
    fn a_follow_reads_from_the_entry_its_reader_holds() {
        let folder = tempfile::tempdir().expect("a temp folder");
        let path = folder.path().join("a3f9c1.jsonl");
        let text: String = (0..10)
            .map(|n| format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"entry {n}\"}}}}\n"))
            .collect();
        fs::write(&path, text).expect("a transcript");
        let kept = Transcripts::new(2);
        let read = |last, from| {
            let excerpt = newest_from(&path, last, from, &kept).expect("read").excerpt;
            let texts: Vec<String> = excerpt.entries.into_iter().map(|e| e.text).collect();
            (excerpt.earlier, texts)
        };

        assert_eq!(
            read(5000, 7),
            (
                7,
                vec!["entry 7".into(), "entry 8".into(), "entry 9".into()]
            )
        );
        assert_eq!(read(1, 7), (9, vec!["entry 9".into()]));
        assert_eq!(read(5000, 10), (10, vec![]));
        // A start the transcript no longer reaches reads as no start at all.
        assert_eq!(read(3, 50).0, 7);
    }

    #[test]
    fn pages_before_an_entry_cover_a_claude_transcript_without_a_gap_or_a_repeat() {
        let folder = tempfile::tempdir().expect("a temp folder");
        let path = folder.path().join("a3f9c1.jsonl");
        fs::write(&path, FIXTURE).expect("a transcript");
        let kept = Transcripts::new(2);
        let all = entries(&path, 100, None, &kept).expect("read").excerpt;
        assert_eq!(all.earlier, 0);
        assert_eq!(all.entries.len(), 11);

        let mut seen = Vec::new();
        let mut before = None;

        loop {
            let page = entries(&path, 3, before, &kept).expect("read").excerpt;

            if let Some(end) = before {
                assert_eq!(page.earlier + page.entries.len() as u64, end);
            }

            let earlier = page.earlier;
            seen.splice(0..0, page.entries);

            if earlier == 0 {
                break;
            }
            before = Some(earlier);
        }

        assert_eq!(seen, all.entries);
    }

    #[test]
    fn imported_automatic_titles_are_cleaned_but_custom_titles_stay() {
        let tagged = tags::from_session("parent", "Parent task", "Fix the upload queue.");
        let mut digest =
            Digest::read(&serde_json::json!({"type": "ai-title", "aiTitle": tagged}).to_string());
        let gist = Gist::of(digest, 0, 0, Vec::new());
        assert_eq!(gist.said.title().as_deref(), Some("Fix the upload queue."));

        let custom = "Custom title ".repeat(10).trim().to_owned();
        digest = Digest::read(
            &serde_json::json!({"type": "custom-title", "customTitle": custom}).to_string(),
        );
        let gist = Gist::of(digest, 0, 0, Vec::new());
        assert_eq!(gist.said.title().as_deref(), Some(custom.as_str()));
    }

    #[test]
    fn a_resume_in_the_launch_folder_keeps_the_worktree_its_tools_ran_in() {
        let line = |cwd: &str, content: Value| {
            serde_json::json!({"type": "user", "cwd": cwd, "message": {"role": "user", "content": content}})
                .to_string()
        };
        let tool = serde_json::json!([{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]);
        let lines = [
            line("/srv/project", "start".into()),
            line("/srv/project/.worktrees/fix", tool.clone()),
            line("/srv/project", "how does it work?".into()),
        ];
        let digest = Digest::read(&lines.join("\n"));
        assert_eq!(digest.said.cwd.as_deref(), Some("/srv/project/.worktrees/fix"));
        assert_eq!(digest.said.launched.as_deref(), Some("/srv/project"));

        let back = [lines.join("\n"), line("/srv/project", tool)].join("\n");
        assert_eq!(Digest::read(&back).said.cwd.as_deref(), Some("/srv/project"));
    }

    #[test]
    fn a_transcript_reads_as_prompts_text_and_tool_lines() {
        let digest = Digest::read(FIXTURE);
        assert_eq!(digest.said.cwd.as_deref(), Some("/srv/project"));
        assert_eq!(digest.said.title().as_deref(), Some("Fix the flaky test"));
        assert_eq!(digest.said.prompt(), Some("now run the whole suite"));
        assert_eq!(
            digest.said.last_message.as_deref(),
            Some("All 42 tests pass now.")
        );
        assert_eq!(digest.turns_ended, 2);
        let shown: Vec<(EntryKind, &str)> = digest
            .entries
            .iter()
            .map(|entry| (entry.kind, entry.text.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                (EntryKind::User, "fix the flaky test in tests/net.rs"),
                (EntryKind::Thought, "secret reasoning"),
                (EntryKind::Agent, "Looking at the test first."),
                (EntryKind::Tool, "Read: /srv/project/tests/net.rs"),
                (EntryKind::Tool, "Bash: cargo test net (failed)"),
                (EntryKind::Agent, "The test raced the server. Fixed it."),
                (EntryKind::User, "/compact keep the test names"),
                (
                    EntryKind::Compacted,
                    "[conversation compacted]\nThis session is being continued from a previous conversation."
                ),
                (EntryKind::User, "now run the whole suite"),
                (EntryKind::Tool, "Bash: cargo test"),
                (EntryKind::Agent, "All 42 tests pass now."),
            ]
        );
        assert_eq!(digest.first_at, Some(1_793_260_800));
        // Its thinking took from the prompt's line to its own.
        assert_eq!(digest.entries[1].ms, Some(5000));
    }

    #[test]
    fn thinking_blocks_read_as_thoughts_that_say_how_long_they_took() {
        let line = |at: &str, content: serde_json::Value| {
            serde_json::json!({"type": "assistant", "timestamp": at,
                "message": {"content": content}})
            .to_string()
        };
        let prompt = serde_json::json!({"type": "user", "timestamp": "2026-10-29T08:00:00.250Z",
            "message": {"role": "user", "content": "is 391 prime?"}});
        let text = [
            prompt.to_string(),
            line(
                "2026-10-29T08:00:03.000Z",
                serde_json::json!([{"type": "thinking", "thinking": "Try 17.", "signature": "sig"}]),
            ),
            line(
                "2026-10-29T08:00:04.500Z",
                serde_json::json!([{"type": "thinking", "thinking": "**Found it**\n\n17 x 23 = 391.", "signature": "sig"}]),
            ),
            line(
                "2026-10-29T08:00:05.000Z",
                serde_json::json!([{"type": "text", "text": "No: 17 x 23."}]),
            ),
            // Thinking display `omitted` keeps only the signature.
            line(
                "2026-10-29T08:00:09.000Z",
                serde_json::json!([{"type": "thinking", "thinking": "", "signature": "sig"},
                    {"type": "redacted_thinking", "data": "opaque"}]),
            ),
            line(
                "2026-10-29T08:00:10.000Z",
                serde_json::json!([{"type": "text", "text": "Anything else?"}]),
            ),
        ]
        .join("\n");
        let digest = Digest::read(&text);
        let shown: Vec<(EntryKind, &str, Option<u64>)> = digest
            .entries
            .iter()
            .map(|entry| (entry.kind, entry.text.as_str(), entry.ms))
            .collect();
        assert_eq!(
            shown,
            [
                (EntryKind::User, "is 391 prime?", None),
                (
                    EntryKind::Thought,
                    "Try 17.\n\n**Found it**\n\n17 x 23 = 391.",
                    Some(4250)
                ),
                (EntryKind::Agent, "No: 17 x 23.\n\nAnything else?", None),
            ]
        );
    }

    #[test]
    fn a_tail_keeps_only_whole_lines() {
        let dir = std::env::temp_dir().join(format!("beep-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let mut file = File::create(&path).unwrap();
        write!(file, "first line\nsecond line\nthird, not ended").unwrap();
        // 39 bytes: the last 20 start inside the second line.
        let tail = read_tail(&path, 20).unwrap();
        assert_eq!(tail.text, "");
        assert!(!tail.whole);
        let tail = read_tail(&path, 28).unwrap();
        assert_eq!(tail.text, "second line\n");
        assert_eq!(tail.end, 23);
        // A start on a line's first byte keeps that line.
        let mut file = File::open(&path).unwrap();
        let tail = read_from(&mut file, 11, 40).unwrap();
        assert_eq!(tail.text, "second line\n");
        let tail = read_tail(&path, 1000).unwrap();
        assert_eq!(tail.text, "first line\nsecond line\n");
        assert!(tail.whole);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tool_result_with_an_image_shows_its_image_line_on_the_tool() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let png = b"\x89PNG\r\n\x1a\nscreen".to_vec();
        let used = serde_json::json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/srv/project/shot.png"}}
        ]}});
        let result = serde_json::json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png",
                    "data": STANDARD.encode(&png)}}
            ]}
        ]}});
        let text = format!("{used}\n{result}\n");
        let digest = Digest::with_pictures(&text);
        let shown = format!("[image: image-{}]", beep_proto::image_key(&png));
        assert_eq!(digest.entries.len(), 1);
        assert!(
            digest.entries[0].text.ends_with(&format!("\n{shown}")),
            "{}",
            digest.entries[0].text
        );
        assert_eq!(digest.pictures.unwrap()[0].data, png);
    }

    /// The user entry a single user line gives.
    fn user_entry(content: serde_json::Value) -> String {
        let line =
            serde_json::json!({"type": "user", "message": {"role": "user", "content": content}});
        let digest = Digest::with_pictures(&line.to_string());
        assert_eq!(digest.entries.len(), 1, "{:?}", digest.entries);
        assert_eq!(digest.entries[0].kind, EntryKind::User);
        digest.entries[0].text.clone()
    }

    #[test]
    fn a_subagents_beep_call_reads_by_its_beep_title() {
        let mut digest = Digest {
            native: true,
            ..Digest::default()
        };
        let call = serde_json::json!({"type": "assistant", "message": {"content": [{
            "type": "tool_use", "id": "t1", "name": "mcp__beep__remote_run",
            "input": {"machine": "build-box", "command": "cargo test"}
        }]}});
        digest.line(&call.to_string());
        assert_eq!(digest.entries[0].kind, EntryKind::Tool);
        assert!(
            digest.entries[0]
                .text
                .contains("Run on build-box: cargo test"),
            "{}",
            digest.entries[0].text
        );
    }

    #[test]
    fn a_cut_off_turn_reads_as_a_note_not_a_prompt() {
        for content in [
            serde_json::json!("[Request interrupted by user]"),
            serde_json::json!([{"type": "text", "text": "[Request interrupted by user for tool use]"}]),
        ] {
            let line = serde_json::json!({"type": "user", "message": {"role": "user", "content": content}});
            let digest = Digest::with_pictures(&line.to_string());
            let shown: Vec<_> = digest
                .entries
                .iter()
                .map(|entry| (entry.kind, entry.text.as_str()))
                .collect();
            assert_eq!(shown, [(EntryKind::Other, "Interrupted")]);
            assert_eq!(digest.said.prompt(), None);
        }
    }

    #[test]
    fn a_bare_brief_block_is_not_the_users_text() {
        let brief = crate::agents::tags::tests::BARE_BRIEF;
        let text = user_entry(serde_json::json!([
            {"type": "text", "text": brief},
            {"type": "text", "text": "ok, let's keep going"}
        ]));
        assert_eq!(tags::said(&text), "ok, let's keep going");
        assert_eq!(
            tags::parts(&text),
            [Part::Context(brief), Part::User("ok, let's keep going")]
        );
        // Glued into one string, as some harnesses keep it.
        let text = user_entry(serde_json::json!(format!("{brief}ok, let's keep going")));
        assert_eq!(tags::said(&text), "ok, let's keep going");
    }

    #[test]
    fn a_tagged_line_splits_into_its_three_parts() {
        let text = user_entry(serde_json::json!([
            {"type": "text", "text": tags::context("Beep: the user's standing rules:\n- Be brief.")},
            {"type": "text", "text": tags::from_session("p1", "Release prep", "Check the changelog.")},
            {"type": "text", "text": "ok, let's keep going"}
        ]));
        assert_eq!(
            tags::parts(&text),
            [
                Part::Context("Beep: the user's standing rules:\n- Be brief."),
                Part::Session {
                    id: "p1".into(),
                    name: "Release prep".into(),
                    text: "Check the changelog.",
                },
                Part::User("ok, let's keep going"),
            ]
        );
    }

    #[test]
    fn a_long_transcript_reads_its_newest_page_first_then_pages_back() {
        let folder = tempfile::tempdir().expect("a temp folder");
        let path = folder.path().join("b7e2d4.jsonl");
        let turns = 500;
        let lines: Vec<String> = (0..turns)
            .flat_map(|turn| {
                let id = format!("t{turn}");
                [
                    serde_json::json!({"type": "user", "message": {"role": "user", "content": format!("prompt {turn}")}}),
                    serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": "cargo test"}}]}}),
                    serde_json::json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": "ok", "is_error": false}]}}),
                    serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": format!("done {turn}")}]}}),
                ]
            })
            .map(|line| line.to_string())
            .collect();
        fs::write(&path, lines.join("\n") + "\n").expect("a transcript");
        let kept = Transcripts::new(2);

        let newest = entries(&path, 200, None, &kept).expect("read").excerpt;
        assert_eq!(newest.entries.len(), 200);
        let total = newest.earlier + 200;
        assert_eq!(
            newest.entries.last().map(|e| e.text.as_str()),
            Some(format!("done {}", turns - 1).as_str())
        );

        let mut before = newest.earlier;
        let mut pages = 1;

        while before > 0 {
            let page = entries(&path, 200, Some(before), &kept).expect("read").excerpt;
            assert_eq!(page.earlier + page.entries.len() as u64, before);
            before = page.earlier;
            pages += 1;
        }

        assert_eq!(pages, total.div_ceil(200));
        let first = entries(&path, 1, Some(1), &kept).expect("read").excerpt;
        assert_eq!(first.entries[0].text, "prompt 0");
    }

    #[test]
    fn an_append_to_a_large_transcript_parses_only_the_new_lines() {
        use std::io::{Seek, SeekFrom};
        let folder = tempfile::tempdir().expect("a temp folder");
        let path = folder.path().join("c4d8e1.jsonl");
        let filler = "x".repeat(2000);
        let line = |turn: usize| {
            serde_json::json!({"type": "user", "message": {"role": "user",
                "content": format!("prompt {turn:05} {filler}")}})
            .to_string()
        };
        // About 8 MiB of fabricated turns.
        let text: String = (0..4000).map(|turn| line(turn) + "\n").collect();
        fs::write(&path, &text).expect("a transcript");
        let kept = Transcripts::new(2);
        let first = entries(&path, 200, None, &kept).expect("read").excerpt;
        assert_eq!((first.earlier, first.entries.len()), (3800, 200));

        // Rewrite an early prompt in place: a parse of the whole tail
        // would see it, a parse of the appended lines does not.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open");
        let at = text.find("prompt 03900").expect("an early prompt");
        file.seek(SeekFrom::Start(at as u64)).expect("seek");
        file.write_all(b"PROMPT").expect("rewrite");
        drop(file);
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        writeln!(file, "{}", line(4000)).expect("append");
        drop(file);

        let next = entries(&path, 200, None, &kept).expect("read").excerpt;
        assert_eq!((next.earlier, next.entries.len()), (3801, 200));
        assert!(next.entries[99].text.starts_with("prompt 03900"));
        assert!(next.entries[199].text.starts_with("prompt 04000"));

        // A file written again before the parsed end is parsed again.
        fs::write(&path, text.replace("prompt 03999", "PROMPT 03999")).expect("rewrite");
        let again = entries(&path, 1, None, &kept).expect("read").excerpt;
        assert!(again.entries[0].text.starts_with("PROMPT 03999"));
    }

    #[test]
    fn a_file_moved_over_the_transcript_is_parsed_again() {
        let folder = tempfile::tempdir().expect("a temp folder");
        let path = folder.path().join("c4d8e1.jsonl");
        let text = |first: &str| {
            [first, "second", "third"]
                .map(|prompt| {
                    serde_json::json!({"type": "user", "message": {"role": "user", "content": prompt}})
                        .to_string()
                        + "\n"
                })
                .concat()
        };
        fs::write(&path, text("first")).expect("a transcript");
        let modified = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .expect("a time");
        let kept = Transcripts::new(2);
        let before = entries(&path, 3, None, &kept).expect("read").excerpt;
        assert_eq!(before.entries[0].text, "first");

        // The same length, time and last bytes, in another file.
        let replacement = folder.path().join("c4d8e1.jsonl.tmp");
        fs::write(&replacement, text("FIRST")).expect("a replacement");
        fs::File::options()
            .write(true)
            .open(&replacement)
            .and_then(|file| file.set_modified(modified))
            .expect("its time");
        fs::rename(&replacement, &path).expect("moved over");

        let after = entries(&path, 3, None, &kept).expect("read").excerpt;
        assert_eq!(after.entries[0].text, "FIRST");
    }
}

#[cfg(test)]
#[path = "../../tests/agents_claude_history.rs"]
mod history_tests;
