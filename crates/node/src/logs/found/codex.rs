//! Codex's sessions (`sessions`, `title`):
//! `<CODEX_HOME>/sessions/<yyyy>/<mm>/<dd>/rollout-*.jsonl`, with the titles
//! the user gave in `<CODEX_HOME>/session_index.jsonl`.

use super::common::{Agent, Memos, Seen, header_sessions, prompt_title, unix_seconds, unix_secs};
use super::folder_place::call_folder;
use crate::logs::subagent::Conversation;
use crate::logs::window::{MAX_HEAD, content_text, head_lines, tail_lines};
use beep_proto::SessionId;
use serde_json::Value;
use std::{
    collections::HashMap,
    convert::Infallible,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The longest title index read.
const MAX_INDEX: u64 = 8 * 1024 * 1024;
/// The most of a rollout's end read for the folder it works in.
const MAX_TAIL: u64 = 2 * 1024 * 1024;

/// Codex's folder, when it has sessions here.
pub(crate) fn locate() -> Option<PathBuf> {
    let root = configured()?;
    (root.join("sessions").is_dir() || root.join("archived_sessions").is_dir()).then_some(root)
}

pub(crate) fn configured() -> Option<PathBuf> {
    let root = match env::var_os("CODEX_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?.join(".codex"),
    };
    Some(root)
}

/// Whether `path` is a Codex rollout file.
pub(crate) fn is_rollout(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
}

/// The newest sessions under `root`, sub-agents left out.
pub(crate) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    sessions_in(root, memos, "sessions", 3)
}

pub(crate) fn archived(root: &Path, memos: &Memos) -> Vec<Seen> {
    sessions_in(root, memos, "archived_sessions", 0)
}

fn sessions_in(root: &Path, memos: &Memos, folder: &str, depth: usize) -> Vec<Seen> {
    let index = root.join("session_index.jsonl");
    let Ok(names) = memos
        .names
        .read(&index, || Ok::<_, Infallible>(Arc::new(titles(&index))));
    header_sessions(&root.join(folder), depth, &is_rollout, &memos.heads, header)
        .map(|mut seen| {
            seen.title = names.get(&seen.id).cloned();
            seen
        })
        .collect()
}

/// The session a rollout's first line (`session_meta`) names; `None` for a
/// sub-agent's (its `source` is an object) or a line that is not a header.
fn header(line: &str) -> Option<Seen> {
    header_with_children(line, false)
}

fn header_with_children(line: &str, children: bool) -> Option<Seen> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;

    if value["type"] != "session_meta" {
        return None;
    }

    let meta = &value["payload"];

    if !children && meta["source"].is_object() {
        return None;
    }

    let id = meta["id"].as_str().filter(|id| !id.is_empty())?;
    let cwd = meta["cwd"].as_str().filter(|cwd| !cwd.is_empty())?;
    let started = meta["timestamp"]
        .as_str()
        .or(value["timestamp"].as_str())
        .and_then(unix_seconds)
        .unwrap_or(0);
    Some(Seen {
        agent: Agent::Codex,
        id: Into::<SessionId>::into(id),
        cwd: PathBuf::from(cwd),
        started_at: started,
        last_activity: started,
        title: None,
        file: None,
    })
}

/// Explicit native lookups do not add child threads to the top-level listing.
pub(crate) fn native(root: &Path, id: &SessionId) -> Option<Seen> {
    let Conversation::Codex(thread) = Conversation::try_from(id.as_ref()).ok()? else {
        return None;
    };
    let file = super::codex_run::rollout(root, thread)?;
    let line = super::common::first_line(&file).ok()??;
    let mut seen = header_with_children(&line, true)?;

    if seen.id != thread {
        return None;
    }

    let value: Value = serde_json::from_str(&line).ok()?;

    if !value["payload"]["source"].is_object() {
        return None;
    }

    seen.id = id.to_owned();
    seen.title = value["payload"]["agent_path"]
        .as_str()
        .and_then(|path| path.rsplit('/').next())
        .and_then(prompt_title);
    seen.last_activity = unix_secs(std::fs::metadata(&file).ok()?.modified().ok()?);
    seen.file = Some(file);
    Some(seen)
}

/// The names the user gave sessions, by id, from the index at `index`; a
/// later line wins.
fn titles(index: &Path) -> HashMap<SessionId, String> {
    let Ok(lines) = head_lines(index, MAX_INDEX) else {
        return HashMap::new();
    };
    let mut names = HashMap::new();

    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if let (Some(id), Some(name)) = (value["id"].as_str(), value["thread_name"].as_str())
            && let Some(name) = prompt_title(name)
        {
            names.insert(Into::<SessionId>::into(id), name);
        }
    }

    names
}

/// The first prompt in the rollout at `path`.
pub(crate) fn title(path: &Path) -> Option<String> {
    head_lines(path, MAX_HEAD)
        .ok()?
        .find_map(|line| prompt(&line))
}

/// The folder the rollout at `path` last worked in: the `cwd` of its last
/// `turn_context`, or the `workdir` a later tool call gave (relative to the
/// turn folder). Repeated launch contexts preserve the detected folder. `None`
/// when that stretch names none; the header's `cwd` then holds.
pub(crate) fn latest_folder(path: &Path) -> Option<PathBuf> {
    let mut turn: Option<PathBuf> = None;
    let mut folder: Option<PathBuf> = None;

    for line in tail_lines(path, MAX_TAIL).ok()? {
        let is_turn = line.contains("\"turn_context\"");

        if !is_turn && !line.contains("workdir") {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = &value["payload"];
        if is_turn && value["type"] == "turn_context" {
            if let Some(cwd) = payload["cwd"].as_str()
                && !cwd.is_empty()
            {
                let next = PathBuf::from(cwd);
                if turn.as_ref() != Some(&next) || folder.is_none() {
                    folder = Some(next.clone());
                }
                turn = Some(next);
            }
        } else if payload["type"] == "function_call"
            && let Some(args) = payload["arguments"]
                .as_str()
                .and_then(|args| serde_json::from_str::<Value>(args).ok())
            && let Some(dir) =
                call_folder(turn.as_deref().unwrap_or(Path::new("/")), Some(&args), None)
        {
            folder = Some(dir);
        }
    }

    folder
}

/// The first prompt: an `event_msg` of type `user_message`, or an
/// `item_completed` `UserMessage` in newer versions.
fn prompt(line: &str) -> Option<String> {
    if !line.contains("user_message") && !line.contains("UserMessage") {
        return None;
    }

    let value: Value = serde_json::from_str(line).ok()?;
    let payload = &value["payload"];

    match payload["type"].as_str()? {
        "user_message" => prompt_title(payload["message"].as_str()?),
        "item_completed" if payload["item"]["type"] == "UserMessage" => {
            let content = &payload["item"]["content"];
            content
                .is_array()
                .then(|| content_text(content))
                .flatten()
                .as_deref()
                .and_then(prompt_title)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../tests/logs_found_codex.rs"]
mod tests;
