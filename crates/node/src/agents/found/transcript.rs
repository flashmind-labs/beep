//! A transcript read from the files Codex and pi keep (`Stores::transcript`):
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
        let Some(seen) = self.sessions(None).into_iter().find(|seen| seen.id == id) else {
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
            (agent, _) => bail!(
                "{}'s transcript cannot be read yet, only its title and folder",
                agent.as_str()
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
    let mut entries = Vec::new();
    for value in lines(path)? {
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
                codex_run::call_title(payload).and_then(|title| entry(EntryKind::Tool, &title))
            }
            (Some("compacted"), _) => entry(EntryKind::Other, "[conversation compacted]"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    /// The user entries the file of `lines` gives `read`.
    fn users(lines: &[Value], read: fn(&Path) -> io::Result<Vec<Entry>>) -> Vec<String> {
        let dir = tempfile::tempdir().expect("a temp folder");
        let path = dir.path().join("session.jsonl");
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        fs::write(&path, text.join("\n") + "\n").expect("a session file");
        read(&path)
            .expect("the session reads")
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::User)
            .map(|entry| entry.text)
            .collect()
    }

    #[test]
    fn a_pi_prompt_keeps_the_brief_apart() {
        let brief = crate::agents::mcp::BRIEF;
        let line = json!({"type": "message", "message": {"role": "user", "content": [
            {"type": "text", "text": brief},
            {"type": "text", "text": "ok, let's keep going"}
        ]}});
        let shown = users(&[line], pi);
        assert_eq!(shown.len(), 1);
        assert_eq!(
            tags::parts(&shown[0]),
            [
                tags::Part::Context(brief),
                tags::Part::User("ok, let's keep going")
            ]
        );
    }

    #[test]
    fn a_codex_prompt_keeps_the_brief_apart() {
        let brief = crate::agents::mcp::BRIEF;
        let glued = json!({"type": "event_msg", "payload": {"type": "user_message",
            "message": format!("{brief}\nok, let's keep going")}});
        let item = json!({"type": "event_msg", "payload": {"type": "item_completed",
        "item": {"type": "UserMessage", "content": [
            {"type": "text", "text": brief},
            {"type": "text", "text": "and then?"}
        ]}}});
        let shown = users(&[glued, item], codex);
        let said: Vec<String> = shown.iter().map(|text| tags::said(text)).collect();
        assert_eq!(said, ["ok, let's keep going", "and then?"]);
    }
}
