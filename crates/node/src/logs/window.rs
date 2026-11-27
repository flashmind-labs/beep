//! Which entries of a transcript a read answers with, and bounded JSONL
//! line and text helpers shared across session stores.

use beep_proto::node::agent::Entry;
use crate::logs::picture::Picture;
use serde_json::Value;
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    ops::Range,
    path::Path,
};

/// The positions, among `len` entries whose first has the index `first`,
/// of the `last` entries before the entry with index `before` (the end of
/// the transcript when `None`). Positions before the first entry or past
/// the end are cut off, so a `before` the node no longer reaches gives an
/// empty range.
pub(crate) fn window(first: u64, len: usize, last: usize, before: Option<u64>) -> Range<usize> {
    let end = before
        .map_or(len, |at| {
            usize::try_from(at.saturating_sub(first)).unwrap_or(usize::MAX)
        })
        .min(len);
    end.saturating_sub(last)..end
}

/// How much of a session file is read for its first prompt or its name.
pub(crate) const MAX_HEAD: u64 = 1024 * 1024;

/// Bounds a caller's listing `limit` to `1..=max`, using `default` for `0`.
pub(crate) fn clamp_limit(limit: u32, default: usize, max: usize) -> usize {
    usize::try_from(limit).map_or(max, |n| if n == 0 { default } else { n.min(max) })
}

/// Complete lines within the first `max` bytes of `path`.
pub(crate) fn head_lines(
    path: &Path,
    max: u64,
) -> io::Result<impl Iterator<Item = String>> {
    Ok(BufReader::new(File::open(path)?.take(max))
        .lines()
        .map_while(Result::ok))
}

/// Complete lines within the last `max` bytes of `path`; a line cut by the
/// start of that stretch is skipped.
pub(crate) fn tail_lines(
    path: &Path,
    max: u64,
) -> io::Result<impl Iterator<Item = String>> {
    let mut file = File::open(path)?;
    let cut_at = file.metadata()?.len().saturating_sub(max);
    file.seek(SeekFrom::Start(cut_at))?;
    let mut reader = BufReader::new(file.take(max));

    if cut_at > 0 {
        reader.read_until(b'\n', &mut Vec::new())?;
    }

    Ok(reader.lines().map_while(Result::ok))
}

/// A message `content` value as plain text: a string as it is, or the `text`
/// fields of its blocks joined by newlines.
pub(crate) fn content_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let texts: Vec<_> = parts.iter().filter_map(|p| p["text"].as_str()).collect();
            Some(texts.join("\n"))
        }
        _ => None,
    }
}

/// Formats a tool call's name with an optional one-line detail.
pub(crate) fn titled_call(name: &str, detail: Option<&str>) -> String {
    match detail.filter(|d| !d.is_empty()) {
        Some(detail) => format!("{name}: {detail}"),
        None => name.to_owned(),
    }
}

/// Replaces an in-progress native tool call line with its completion status and output.
pub(crate) fn finish_tool_call(
    text: &mut String,
    failed: bool,
    output: &str,
    max: usize,
) {
    let title = text.strip_suffix(" [in_progress]").unwrap_or(text);
    let status = if failed { "failed" } else { "completed" };
    *text = crate::text::cut(&format!("{title} [{status}]\n{output}"), max);
}

/// What the transcript shows for a compaction, as Claude Code's reader does.
pub(crate) const COMPACTED_ENTRY: &str = "[conversation compacted]";
/// Upper bound for a summary carried in one chat entry.
pub(crate) const MAX_SUMMARY: usize = 128 * 1024;
/// Claude Code's tool that publishes a page or document as an artifact.
pub(crate) const ARTIFACT_TOOL: &str = "Artifact";
/// Claude Code's tool that loads a skill.
pub(crate) const SKILL_TOOL: &str = "Skill";

/// pi-acp's first line before the summary it sends for `/compact`.
const PI_COMPACTED: &str = "Compaction completed.";

pub(crate) fn summary_note(text: &str) -> String {
    let summary = match text.split_once("\nSummary:\n") {
        Some((_, summary)) => summary,
        // pi-acp heads it with the tokens before, then a blank line.
        None if text.starts_with(PI_COMPACTED) => text.split_once("\n\n").map_or("", |(_, s)| s),
        None => text,
    };
    format!(
        "{COMPACTED_ENTRY}\n{}",
        crate::text::cut_str(summary.trim(), MAX_SUMMARY)
    )
}

#[cfg(test)]
#[path = "../tests/logs_window.rs"]
mod tests;

/// The end of a transcript, as `entries` reads it.
#[derive(Debug)]
pub(crate) struct Excerpt {
    pub(crate) entries: Vec<Entry>,
    /// Entries before these in what was read.
    pub(crate) earlier: u64,
    /// The pictures the prompts read held, oldest first.
    pub(crate) pictures: Vec<Picture>,
}
