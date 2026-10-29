//! Claude Code's transcripts: one JSON object per line in
//! `<config>/projects/<folder slug>/<session id>.jsonl`. A long session's
//! file runs to tens of megabytes, mostly request records, so only its tail
//! is read, and only prompts, the agent's text, tool calls and turn ends
//! are kept.

use crate::agents::cut;
use beep_proto::node::agent::{Entry, EntryKind};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
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
    pub(super) entries: Vec<Entry>,
    /// The entry of each tool call, by its id, to mark the failed ones.
    tools: HashMap<String, usize>,
    /// Turns that ended in this stretch.
    pub(super) turns_ended: u32,
}

impl Digest {
    /// Reads every complete line of `text`.
    pub(super) fn read(text: &str) -> Self {
        let mut digest = Self::default();
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
        if let Some(at) = value["timestamp"].as_str().and_then(unix_seconds) {
            self.first_at.get_or_insert(at);
            self.last_at = Some(at);
        }
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
        let text = match content {
            Value::String(text) => Some(text.clone()),
            Value::Array(blocks) => {
                for block in blocks {
                    if block["type"] == "tool_result" && block["is_error"].as_bool() == Some(true) {
                        self.tool_failed(block["tool_use_id"].as_str().unwrap_or(""));
                    }
                }
                let texts: Vec<&str> = blocks
                    .iter()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .collect();
                (!texts.is_empty()).then(|| texts.join("\n"))
            }
            _ => None,
        };
        let Some(prompt) = text.as_deref().and_then(prompt_text) else {
            return;
        };
        self.last_prompt = Some(cut(prompt.trim(), MAX_SHORT));
        self.push(EntryKind::User, prompt.trim());
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
                Some("tool_use") => {
                    let line = tool_line(block);
                    self.push(EntryKind::Tool, &line);
                    if let Some(id) = block["id"].as_str() {
                        self.tools.insert(id.to_owned(), self.entries.len() - 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn tool_failed(&mut self, id: &str) {
        if let Some(entry) = self
            .tools
            .get(id)
            .and_then(|&index| self.entries.get_mut(index))
        {
            entry.text.push_str(" (failed)");
        }
    }

    fn push(&mut self, kind: EntryKind, text: &str) {
        self.entries.push(Entry {
            kind,
            text: cut(text, MAX_ENTRY),
        });
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

/// A tool call as one line, such as `Bash: cargo test`.
fn tool_line(block: &Value) -> String {
    let name = block["name"].as_str().unwrap_or("tool");
    let input = &block["input"];
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
fn unix_seconds(text: &str) -> Option<u64> {
    let (date, time) = text.split_once('T')?;
    let mut date = date.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let time = time.trim_end_matches('Z');
    let mut time = time.split(':');
    let hour: i64 = time.next()?.parse().ok()?;
    let minute: i64 = time.next()?.parse().ok()?;
    let second: i64 = time.next()?.split('.').next()?.parse().ok()?;
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
    u64::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}

/// The complete lines at the end of a file.
pub(super) struct Tail {
    pub(super) text: String,
    /// The text starts at the start of the file.
    pub(super) whole: bool,
    /// Where the text ends in the file: after its last newline.
    pub(super) end: u64,
}

/// Reads the complete lines in the last `max` bytes of `path`.
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
pub(super) fn digest_tail(path: &Path, entries: usize) -> io::Result<(Digest, bool)> {
    let mut window = FIRST_WINDOW;
    loop {
        let tail = read_tail(path, window)?;
        let digest = Digest::read(&tail.text);
        let enough = digest.entries.len() > entries && digest.prompt().is_some();
        if tail.whole || enough || window >= MAX_WINDOW {
            return Ok((digest, tail.whole));
        }
        window *= 4;
    }
}

/// When the session started, from the first timestamp in the file's head.
pub(super) fn started_at(path: &Path) -> io::Result<Option<u64>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let head = read_from(&mut file, 0, len.min(HEAD))?;
    Ok(Digest::read(&head.text).first_at)
}

/// The last `last` entries of the transcript at `path`, and how many came
/// before them in what was read.
pub(super) fn entries(path: &Path, last: usize) -> io::Result<(Vec<Entry>, u64)> {
    let (mut digest, _) = digest_tail(path, last)?;
    let skip = digest.entries.len().saturating_sub(last);
    let entries = digest.entries.split_off(skip);
    Ok((entries, skip as u64))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::Write;

    /// A fabricated transcript in Claude Code's format.
    pub(crate) const FIXTURE: &str = include_str!("fixtures/session.jsonl");

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
    }

    #[test]
    fn timestamps_read_as_unix_seconds() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            unix_seconds("2026-10-29T00:00:00.123Z"),
            Some(1_793_232_000)
        );
        assert_eq!(unix_seconds("2000-03-01T12:00:00Z"), Some(951_912_000));
        assert_eq!(unix_seconds("yesterday"), None);
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
    fn the_last_entries_say_how_many_came_before() {
        let dir = std::env::temp_dir().join(format!("beep-entries-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        std::fs::write(&path, FIXTURE).unwrap();
        let (entries, earlier) = entries(&path, 3).unwrap();
        assert_eq!(earlier, 7);
        assert_eq!(entries[2].text, "All 42 tests pass now.");
        assert_eq!(started_at(&path).unwrap(), Some(1_793_260_800));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
