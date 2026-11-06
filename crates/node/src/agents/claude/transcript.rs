//! Claude Code's transcripts, `<config>/projects/<folder slug>/<id>.jsonl`,
//! digested into prompts, text, thinking, tool calls and turn ends (`Digest`,
//! `digest_tail`, `head`).

use super::subagents;
use crate::{
    agents::mcp::calls::Tool as BeepTool,
    agents::tags::{self, Part},
    agents::{Picture, block_picture, cut, found::memo::Memo, window::window},
};
use beep_proto::node::agent::{Entry, EntryKind};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
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

/// What a stretch of transcript says.
#[derive(Debug, Default)]
pub(super) struct Digest {
    pub(super) cwd: Option<String>,
    pub(super) title: Option<String>,
    /// A title the user set, which wins over the generated one.
    custom_title: Option<String>,
    pub(super) last_prompt: Option<String>,
    /// Claude Code's own record of the last prompt, for when the prompt
    /// itself is before the window.
    recorded_prompt: Option<String>,
    pub(super) last_message: Option<String>,
    /// Unix seconds of the first and last timestamped lines.
    pub(super) first_at: Option<u64>,
    pub(super) last_at: Option<u64>,
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
#[derive(Debug, Default)]
pub(super) struct Spawn {
    /// Its entry among the digest's.
    pub(super) entry: usize,
    /// What it was asked to do, in a few words.
    pub(super) description: Option<String>,
    /// Its `subagent_type`: `Explore`, or a custom agent's name.
    pub(super) agent: Option<String>,
    /// The id the subagent's file is named by, once its result said it.
    pub(super) agent_id: Option<String>,
    pub(super) failed: bool,
}

/// The tools that start a subagent.
const SPAWN_TOOLS: [&str; 2] = ["Agent", "Task"];

/// Claude Code's tool that publishes a page or document as an artifact.
const ARTIFACT_TOOL: &str = "Artifact";

/// Where a published artifact's link starts.
const ARTIFACT_LINK: &str = "https://claude.ai/";

impl Digest {
    /// Reads every complete line of `text`.
    pub(super) fn read(text: &str) -> Self {
        let mut digest = Self::default();
        digest.lines(text);
        digest
    }

    /// Reads every complete line of `text`, keeping prompts' pictures.
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

    /// The title to show: the user's, else the generated one.
    pub(super) fn title(&self) -> Option<&str> {
        self.custom_title.as_deref().or(self.title.as_deref())
    }

    pub(super) fn prompt(&self) -> Option<&str> {
        self.last_prompt
            .as_deref()
            .or(self.recorded_prompt.as_deref())
    }

    pub(super) fn line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            return;
        };
        // A sub-agent's lines belong to its own conversation.
        if value["isSidechain"].as_bool() == Some(true) {
            return;
        }
        let at_ms = value["timestamp"].as_str().and_then(unix_millis);
        if let Some(at_ms) = at_ms {
            let at = at_ms / 1000;
            self.first_at.get_or_insert(at);
            self.last_at = Some(at);
        }
        self.before_ms = self.line_ms.take().or(self.before_ms);
        self.line_ms = at_ms;
        if let Some(cwd) = value["cwd"].as_str() {
            self.cwd = Some(cwd.to_owned());
        }
        match value["type"].as_str().unwrap_or("") {
            "user" => self.user(&value),
            "assistant" => self.assistant(&value),
            "system" => match value["subtype"].as_str() {
                Some("turn_duration") => self.turns_ended += 1,
                Some("compact_boundary") => self.push(EntryKind::Other, "[conversation compacted]"),
                _ => {}
            },
            "ai-title" => self.title = text_field(&value, "aiTitle"),
            "custom-title" => self.custom_title = text_field(&value, "customTitle"),
            "summary" => self.title = self.title.take().or(text_field(&value, "summary")),
            "last-prompt" => self.recorded_prompt = text_field(&value, "lastPrompt"),
            _ => {}
        }
    }

    fn user(&mut self, value: &Value) {
        if value["isMeta"].as_bool() == Some(true)
            || value["isCompactSummary"].as_bool() == Some(true)
        {
            return;
        }
        // Prompts other than the user's own, such as a finished task's notice.
        if let Some(kind) = value["origin"]["kind"].as_str()
            && kind != "human"
        {
            return;
        }
        let content = &value["message"]["content"];
        let mut lines = String::new();
        let text = match content {
            Value::String(text) => Some(tags::untagged(text)),
            Value::Array(blocks) => {
                for block in blocks.iter().filter(|block| block["type"] == "tool_result") {
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    if block["is_error"].as_bool() == Some(true) {
                        self.tool_failed(id);
                    } else if self.artifacts.remove(id) {
                        self.artifact_published(id, &block["content"]);
                    }
                    if let Some(spawn) = self.spawns.get_mut(id)
                        && let Some(agent_id) = value["toolUseResult"]["agentId"].as_str()
                    {
                        spawn.agent_id = Some(agent_id.to_owned());
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
                Part::Session { text, .. } => Some(*text),
                _ => None,
            })
        });
        if let Some(said) = said {
            self.last_prompt = Some(cut(said.trim(), MAX_SHORT));
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
                    self.last_message = Some(cut(text, MAX_SHORT));
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
                    let line = tool_line(block);
                    self.push(EntryKind::Tool, &line);
                    let entry = self.entries.len() - 1;
                    let Some(id) = id else {
                        continue;
                    };
                    self.tools.insert(id.to_owned(), entry);
                    if block["name"].as_str() == Some(ARTIFACT_TOOL) {
                        self.artifacts.insert(id.to_owned());
                    }
                    if block["name"]
                        .as_str()
                        .is_some_and(|name| SPAWN_TOOLS.contains(&name))
                    {
                        let input = &block["input"];
                        self.spawns.insert(
                            id.to_owned(),
                            Spawn {
                                entry,
                                description: text_field(input, "description"),
                                agent: text_field(input, "subagent_type"),
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

    fn tool_failed(&mut self, id: &str) {
        if let Some(spawn) = self.spawns.get_mut(id) {
            spawn.failed = true;
        }
        if let Some(entry) = self
            .tools
            .get(id)
            .and_then(|&index| self.entries.get_mut(index))
        {
            entry.text.push_str(" (failed)");
        }
    }

    /// Writes the link an `Artifact` call's result gives after its entry,
    /// as a live session writes a finished call: `Artifact: <path>
    /// [completed]`, then the link.
    fn artifact_published(&mut self, id: &str, content: &Value) {
        let text = match content {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => return,
        };
        let Some(link) = artifact_link(&text) else {
            return;
        };
        if let Some(entry) = self
            .tools
            .get(id)
            .and_then(|&index| self.entries.get_mut(index))
        {
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

/// Unix seconds of an RFC 3339 UTC time such as `2026-10-29T12:34:56.789Z`.
pub(crate) fn unix_seconds(text: &str) -> Option<u64> {
    unix_millis(text).map(|ms| ms / 1000)
}

/// Unix milliseconds of an RFC 3339 UTC time such as
/// `2026-10-29T12:34:56.789Z`.
fn unix_millis(text: &str) -> Option<u64> {
    let (date, time) = text.split_once('T')?;
    let mut date = date.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let time = time.trim_end_matches('Z');
    let mut time = time.split(':');
    let hour: i64 = time.next()?.parse().ok()?;
    let minute: i64 = time.next()?.parse().ok()?;
    let seconds = time.next()?;
    let (second, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second: i64 = second.parse().ok()?;
    // Milliseconds from the first three digits of the fraction.
    let mut millis: i64 = 0;
    for (i, digit) in fraction.chars().take(3).enumerate() {
        let digit = i64::from(digit.to_digit(10)?);
        millis += digit * [100, 10, 1][i];
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970-01-01, from Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from((days * 86_400 + hour * 3600 + minute * 60 + second) * 1000 + millis).ok()
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
    digest_tail_with(path, entries, Digest::read)
}

fn digest_tail_with(path: &Path, entries: usize, read: fn(&str) -> Digest) -> io::Result<Digest> {
    let mut window = FIRST_WINDOW;
    loop {
        let tail = read_tail(path, window)?;
        let digest = read(&tail.text);
        let enough = digest.entries.len() > entries && digest.prompt().is_some();
        if tail.whole || enough || window >= MAX_WINDOW {
            return Ok(digest);
        }
        window *= 4;
    }
}

/// When the session started, from the first timestamp in the file's head.
pub(super) fn started_at(path: &Path) -> io::Result<Option<u64>> {
    Ok(head(path)?.first_at)
}

/// What the file's head says: where the session started, and when.
pub(super) fn head(path: &Path) -> io::Result<Digest> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let head = read_from(&mut file, 0, len.min(HEAD))?;
    Ok(Digest::read(&head.text))
}

/// The end of a transcript, as `entries` reads it.
#[derive(Debug)]
pub(crate) struct Excerpt {
    pub(crate) entries: Vec<Entry>,
    /// Entries before these in what was read.
    pub(crate) earlier: u64,
    /// The pictures the prompts read held, oldest first.
    pub(crate) pictures: Vec<Picture>,
}

/// The `last` entries before entry `before` (the end when `None`) of the
/// transcript at `path`, with the pictures their image lines show. The
/// file is read as far back as `MAX_WINDOW` once, kept in `kept` while it
/// does not change, so every page counts entries from the same first one.
pub(super) fn entries(
    path: &Path,
    last: usize,
    before: Option<u64>,
    kept: &Memo<Arc<Digest>>,
) -> io::Result<Excerpt> {
    let digest = kept.read(path, || {
        let tail = read_tail(path, MAX_WINDOW)?;
        Ok::<_, io::Error>(Arc::new(Digest::with_pictures(&tail.text)))
    })?;
    let range = window(0, digest.entries.len(), last, before);
    let skip = range.start;
    let mut entries = digest.entries[range].to_vec();
    subagents::attach(path, &digest.spawns, &mut entries, skip);
    Ok(Excerpt {
        entries,
        earlier: skip as u64,
        pictures: digest.pictures.clone().unwrap_or_default(),
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::{fs, io::Write};

    /// A fabricated transcript in Claude Code's format.
    pub(crate) const FIXTURE: &str = include_str!("fixtures/session.jsonl");

    #[test]
    fn pages_before_an_entry_cover_a_claude_transcript_without_a_gap_or_a_repeat() {
        let folder = tempfile::tempdir().expect("a temp folder");
        let path = folder.path().join("a3f9c1.jsonl");
        fs::write(&path, FIXTURE).expect("a transcript");
        let kept = Memo::new(2);
        let all = entries(&path, 100, None, &kept).expect("read");
        assert_eq!(all.earlier, 0);
        assert_eq!(all.entries.len(), 11);

        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let page = entries(&path, 3, before, &kept).expect("read");
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
    fn a_transcript_reads_as_prompts_text_and_tool_lines() {
        let digest = Digest::read(FIXTURE);
        assert_eq!(digest.cwd.as_deref(), Some("/srv/project"));
        assert_eq!(digest.title(), Some("Fix the flaky test"));
        assert_eq!(digest.prompt(), Some("now run the whole suite"));
        assert_eq!(
            digest.last_message.as_deref(),
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
                (EntryKind::Other, "[conversation compacted]"),
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
    fn a_bare_brief_block_is_not_the_users_text() {
        let brief = crate::agents::mcp::brief(false);
        let text = user_entry(serde_json::json!([
            {"type": "text", "text": brief},
            {"type": "text", "text": "ok, let's keep going"}
        ]));
        assert_eq!(tags::said(&text), "ok, let's keep going");
        assert_eq!(
            tags::parts(&text),
            [Part::Context(&brief), Part::User("ok, let's keep going")]
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
}
