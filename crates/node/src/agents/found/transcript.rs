//! A transcript read from the files Codex, pi and flsh keep (`Stores::transcript`):
//! the last entries as the node answers a transcript request with them.

use super::{Agent, Stores, codex_run};
use crate::agents::{claude::Excerpt, cut, tags, window::window};
use anyhow::{Result, bail};
use beep_proto::node::agent::{Entry, EntryKind};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

/// The most of a session file read, from its end.
const MAX_READ: u64 = 32 * 1024 * 1024;
/// The longest entry kept.
const MAX_ENTRY: usize = 4 * 1024;

impl Stores {
    /// The last `last` entries of session `id` in the files these stores
    /// keep; `None` when no store has it. Blocks: run it with
    /// `spawn_blocking`.
    pub(crate) fn transcript(
        &self,
        id: &str,
        last: usize,
        before: Option<u64>,
    ) -> Result<Option<Excerpt>> {
        let Some(seen) = self.seen(None, id) else {
            return Ok(None);
        };
        let entries = match (seen.agent, &seen.file) {
            (Agent::Codex, Some(file)) => self
                .memos
                .transcripts
                .read(file, || codex(file).map(Arc::new))?,
            (Agent::Pi, Some(file)) => self
                .memos
                .transcripts
                .read(file, || pi(file).map(Arc::new))?,
            (Agent::Flsh, Some(file)) => self
                .memos
                .transcripts
                .read(file, || flsh(file).map(Arc::new))?,
            (agent, _) => bail!(
                "{}'s transcript cannot be read yet, only its title and folder",
                agent.as_ref()
            ),
        };
        let range = window(0, entries.len(), last, before);
        Ok(Some(Excerpt {
            earlier: range.start as u64,
            entries: entries[range].to_vec(),
            pictures: Vec::new(),
        }))
    }
}

/// The lines of the last `MAX_READ` bytes of the file at `path`; a line cut
/// by the start of that stretch is left out.
fn lines(path: &Path) -> io::Result<impl Iterator<Item = Value>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let cut_at = len.saturating_sub(MAX_READ);
    file.seek(SeekFrom::Start(cut_at))?;
    let mut reader = BufReader::new(file.take(MAX_READ));
    if cut_at > 0 {
        reader.read_until(b'\n', &mut Vec::new())?;
    }
    Ok(reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok()))
}

fn entry(kind: EntryKind, text: &str) -> Option<Entry> {
    let text = text.trim();
    (!text.is_empty()).then(|| Entry::new(kind, cut(text, MAX_ENTRY)))
}

/// The text of a message's `content` parts.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A prompt's text from its `content`, a bare brief Beep sent ahead of it
/// tagged as Beep's context (`tags`).
fn prompt_of(content: &Value) -> String {
    match content {
        Value::String(text) => tags::untagged(text),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .map(tags::bare_block)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A Codex rollout: prompts from `user_message` events (the `response_item`
/// ones also hold the context Codex adds), its replies, reasoning summaries
/// and calls, named as a subagent's steps are (`codex_run::call_title`).
fn codex(path: &Path) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut calls: HashMap<String, usize> = HashMap::new();
    let head = super::first_line(path)?.unwrap_or_default();
    let meta: Value = serde_json::from_str(&head).unwrap_or_default();
    let native = meta["payload"]["source"].is_object();
    let skip = meta["payload"]["subagent_history_start_ordinal"]
        .as_u64()
        .unwrap_or(0);
    // Forked threads start with copied parent history. Count original lines,
    // including malformed ones, before parsing only the child's own entries.
    let values: Box<dyn Iterator<Item = Value>> = if native {
        Box::new(
            BufReader::new(File::open(path)?.take(MAX_READ))
                .lines()
                .enumerate()
                .filter(move |(at, _)| (*at as u64) >= skip)
                .filter_map(|(_, line)| serde_json::from_str(&line.ok()?).ok()),
        )
    } else {
        Box::new(lines(path)?)
    };
    for value in values {
        let payload = &value["payload"];
        let kind = payload["type"].as_str();
        let made = match (value["type"].as_str(), kind) {
            (Some("event_msg"), Some("user_message")) => entry(
                EntryKind::User,
                &tags::untagged(payload["message"].as_str().unwrap_or_default()),
            ),
            (Some("event_msg"), Some("item_completed"))
                if payload["item"]["type"] == "UserMessage" =>
            {
                entry(EntryKind::User, &prompt_of(&payload["item"]["content"]))
            }
            (Some("response_item"), Some("message")) if payload["role"] == "assistant" => {
                entry(EntryKind::Agent, &text_of(&payload["content"]))
            }
            (Some("response_item"), Some("reasoning")) => {
                entry(EntryKind::Thought, &text_of(&payload["summary"]))
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let title = if native {
                    let name = payload["name"].as_str().unwrap_or("tool");
                    let args = payload["arguments"]
                        .as_str()
                        .and_then(|text| serde_json::from_str::<Value>(text).ok());
                    let detail = args
                        .as_ref()
                        .and_then(|args| {
                            ["cmd", "command", "file_path", "path", "pattern", "query"]
                                .iter()
                                .find_map(|key| args[key].as_str())
                        })
                        .or_else(|| payload["input"].as_str());
                    Some(
                        detail
                            .map_or_else(|| name.to_owned(), |detail| format!("{name}: {detail}")),
                    )
                } else {
                    codex_run::call_title(payload)
                };
                if native && let Some(id) = payload["call_id"].as_str() {
                    calls.insert(id.to_owned(), entries.len());
                }
                title.and_then(|title| {
                    entry(
                        EntryKind::Tool,
                        &if native {
                            format!("{title} [in_progress]")
                        } else {
                            title
                        },
                    )
                })
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output"))
                if native =>
            {
                if let Some(call) = payload["call_id"]
                    .as_str()
                    .and_then(|id| calls.get(id))
                    .and_then(|&at| entries.get_mut(at))
                {
                    let title = call
                        .text
                        .strip_suffix(" [in_progress]")
                        .unwrap_or(&call.text);
                    let output = payload["output"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| payload["output"].to_string());
                    let failed = payload["output"]["exit_code"]
                        .as_i64()
                        .is_some_and(|code| code != 0)
                        || output.lines().any(|line| {
                            ["Process exited with code", "Exit code:"]
                                .iter()
                                .any(|prefix| {
                                    line.trim()
                                        .strip_prefix(prefix)
                                        .and_then(|code| {
                                            code.trim_start_matches([' ', ':'])
                                                .split_whitespace()
                                                .next()
                                        })
                                        .and_then(|code| code.parse::<i32>().ok())
                                        .is_some_and(|code| code != 0)
                                })
                        });
                    call.text = cut(
                        &format!(
                            "{title} [{}]\n{output}",
                            if failed { "failed" } else { "completed" }
                        ),
                        MAX_ENTRY,
                    );
                }
                None
            }
            (Some("compacted"), _) => entry(EntryKind::Compacted, "[conversation compacted]"),
            _ => None,
        };
        entries.extend(made);
    }
    Ok(entries)
}

/// A pi session: `message` lines, each a user prompt, an assistant message
/// of text, thinking and tool calls, or a tool result that marks its call.
fn pi(path: &Path) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut calls: HashMap<String, usize> = HashMap::new();
    for value in lines(path)? {
        if value["type"] != "message" {
            continue;
        }
        let message = &value["message"];
        match message["role"].as_str() {
            Some("user") => entries.extend(entry(EntryKind::User, &prompt_of(&message["content"]))),
            Some("assistant") => {
                let Value::Array(parts) = &message["content"] else {
                    continue;
                };
                for part in parts {
                    let made = match part["type"].as_str() {
                        Some("text") => {
                            entry(EntryKind::Agent, part["text"].as_str().unwrap_or_default())
                        }
                        Some("thinking") => entry(
                            EntryKind::Thought,
                            part["thinking"].as_str().unwrap_or_default(),
                        ),
                        Some("toolCall") => {
                            let name = part["name"].as_str().unwrap_or("tool");
                            let title = match codex_run::detail(&part["arguments"]) {
                                Some(detail) if !detail.is_empty() => format!("{name}: {detail}"),
                                _ => name.to_owned(),
                            };
                            let made = entry(EntryKind::Tool, &title);
                            if let Some(id) = part["id"].as_str() {
                                calls.insert(id.to_owned(), entries.len());
                            }
                            made
                        }
                        _ => None,
                    };
                    entries.extend(made);
                }
            }
            Some("toolResult") if message["isError"] == true => {
                let id = message["toolCallId"].as_str().unwrap_or_default();
                if let Some(call) = calls.get(id).and_then(|&at| entries.get_mut(at)) {
                    call.text.push_str(" (failed)");
                }
            }
            _ => {}
        }
    }
    Ok(entries)
}

/// A flsh session file: one line per message, `user`, `assistant` (its
/// text, `metadata.reasoning` and `toolCalls`) or `tool`.
fn flsh(path: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for value in lines(path)? {
        let message = &value["message"];
        let content = message["content"].as_str().unwrap_or_default();
        match value["type"].as_str() {
            Some("user") => entries.extend(entry(EntryKind::User, &tags::untagged(content))),
            Some("assistant") => {
                let reasoning = message["metadata"]["reasoning"].as_str();
                entries.extend(entry(EntryKind::Thought, reasoning.unwrap_or_default()));
                entries.extend(entry(EntryKind::Agent, content));
                for call in message["toolCalls"].as_array().into_iter().flatten() {
                    let name = call["name"].as_str().unwrap_or("tool");
                    let title = match codex_run::detail(&call["arguments"]) {
                        Some(detail) if !detail.is_empty() => format!("{name}: {detail}"),
                        _ => name.to_owned(),
                    };
                    entries.extend(entry(EntryKind::Tool, &title));
                }
            }
            _ => {}
        }
    }
    Ok(entries)
}

#[cfg(test)]
#[path = "../../tests/agents_found_transcript.rs"]
mod tests;
