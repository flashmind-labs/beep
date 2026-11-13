//! pi's sessions (`locate`, `sessions`, `title`):
//! `<agent dir>/sessions/--<folder>--/<time>_<id>.jsonl`, a `session` header on
//! the first line and a `session_info` line naming it.

use super::{Agent, Memos, Seen, first_line, head_lines, newest_files, prompt_title};
use crate::agents::claude::unix_seconds;
use serde_json::Value;
use std::{
    env,
    path::{Path, PathBuf},
};

/// pi's sessions folder, when it has one here.
pub(super) fn locate() -> Option<PathBuf> {
    configured().filter(|sessions| sessions.is_dir())
}

pub(crate) fn configured() -> Option<PathBuf> {
    let sessions = match env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => agent_dir()?.join("sessions"),
    };
    Some(sessions)
}

/// pi's agent dir, which holds its `auth.json`.
pub(super) fn agent_dir() -> Option<PathBuf> {
    match env::var_os("PI_CODING_AGENT_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => Some(dirs::home_dir()?.join(".pi/agent")),
    }
}

/// The newest sessions in the sessions folder `root`.
pub(super) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let jsonl = |path: &Path| path.extension().is_some_and(|ext| ext == "jsonl");
    newest_files(root, 1, &jsonl)
        .into_iter()
        .filter_map(|(path, at)| {
            let read = || first_line(&path).map(|line| line.as_deref().and_then(header));
            let mut seen = memos.heads.read(&path, read).ok()??;
            seen.last_activity = at;
            seen.file = Some(path);
            Some(seen)
        })
        .collect()
}

fn header(line: &str) -> Option<Seen> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;

    if value["type"] != "session" {
        return None;
    }

    let id = value["id"].as_str().filter(|id| !id.is_empty())?;
    let cwd = value["cwd"].as_str().filter(|cwd| !cwd.is_empty())?;
    let started = value["timestamp"]
        .as_str()
        .and_then(unix_seconds)
        .unwrap_or(0);
    Some(Seen {
        agent: Agent::Pi,
        id: id.to_owned(),
        cwd: PathBuf::from(cwd),
        started_at: started,
        last_activity: started,
        title: None,
        file: None,
    })
}

/// The session's name, else its first prompt.
pub(super) fn title(path: &Path) -> Option<String> {
    let mut first = None;
    let mut name = None;

    for line in head_lines(path).ok()? {
        if !line.contains("\"session_info\"") && (first.is_some() || !line.contains("\"user\"")) {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        match value["type"].as_str() {
            Some("session_info") => {
                if let Some(given) = value["name"].as_str().and_then(prompt_title) {
                    name = Some(given);
                }
            }
            Some("message") if first.is_none() && value["message"]["role"] == "user" => {
                first = prompt(&value["message"]["content"]);
            }
            _ => {}
        }
    }

    name.or(first)
}

/// A message's text: a string, or its text blocks.
fn prompt(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return prompt_title(text);
    }

    let text: Vec<&str> = content
        .as_array()?
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    prompt_title(&text.join("\n"))
}

#[cfg(test)]
#[path = "../../tests/agents_found_pi.rs"]
mod tests;
