//! pi's sessions (`locate`, `sessions`, `title`):
//! `<agent dir>/sessions/--<folder>--/<time>_<id>.jsonl`, a `session` header on
//! the first line and a `session_info` line naming it.

use super::common::{Agent, Memos, Seen, header_sessions, is_jsonl, prompt_title};
use super::common::unix_seconds;
use crate::logs::window::{MAX_HEAD, content_text, head_lines};
use beep_proto::SessionId;
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// pi's sessions folder, when it has one here.
pub(crate) fn locate() -> Option<PathBuf> {
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
pub(crate) fn agent_dir() -> Option<PathBuf> {
    agent_dir_in(&|name| env::var_os(name))
}

/// pi's agent dir with the variables `env` gives.
pub(crate) fn agent_dir_in(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    match env("PI_CODING_AGENT_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => Some(dirs::home_dir()?.join(".pi/agent")),
    }
}

/// The newest sessions in the sessions folder `root`.
pub(crate) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    header_sessions(root, 1, &is_jsonl, &memos.heads, header).collect()
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
        id: Into::<SessionId>::into(id),
        cwd: PathBuf::from(cwd),
        started_at: started,
        last_activity: started,
        title: None,
        file: None,
    })
}

/// The session's name, else its first prompt.
pub(crate) fn title(path: &Path) -> Option<String> {
    let mut first = None;
    let mut name = None;

    for line in head_lines(path, MAX_HEAD).ok()? {
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
                first = content_text(&value["message"]["content"])
                    .as_deref()
                    .and_then(prompt_title);
            }
            _ => {}
        }
    }

    name.or(first)
}

#[cfg(test)]
#[path = "../../tests/logs_found_pi.rs"]
mod tests;
