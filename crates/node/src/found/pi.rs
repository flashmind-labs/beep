//! pi's sessions: `<agent dir>/sessions/--<folder>--/<time>_<id>.jsonl`,
//! the agent dir being `~/.pi/agent` unless `PI_CODING_AGENT_DIR` says
//! otherwise, and the sessions folder moved by `PI_CODING_AGENT_SESSION_DIR`.
//! The first line is the header, `{"type":"session","id","timestamp","cwd"}`;
//! a `session_info` line names the session, and the first `message` whose
//! role is `user` is its first prompt.

use super::{Agent, Memos, Seen, first_line, head_lines, newest_files, prompt_title};
use crate::claude::unix_seconds;
use serde_json::Value;
use std::{
    env,
    path::{Path, PathBuf},
};

/// pi's sessions folder, when it has one here.
pub(super) fn locate() -> Option<PathBuf> {
    let sessions = match env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => agent_dir()?.join("sessions"),
    };
    sessions.is_dir().then_some(sessions)
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
mod tests {
    use super::*;
    use std::fs;

    const ID: &str = "01a04575-2085-7b50-ace5-79c3f3ba9ff3";

    fn session(root: &Path, lines: &[&str]) -> PathBuf {
        let dir = root.join("--home-user-proj--");
        fs::create_dir_all(&dir).expect("folders");
        let path = dir.join(format!("2026-08-27T23-01-39-077Z_{ID}.jsonl"));
        fs::write(&path, lines.join("\n") + "\n").expect("a session");
        path
    }

    const HEADER: &str = r#"{"type":"session","version":3,"id":"01a04575-2085-7b50-ace5-79c3f3ba9ff3","timestamp":"2026-08-27T23:01:39.077Z","cwd":"/home/user/proj"}"#;
    const MODEL: &str = r#"{"type":"model_change","id":"a1","parentId":null,"timestamp":"2026-08-27T23:01:40.000Z","provider":"p","modelId":"m"}"#;
    const USER: &str = r#"{"type":"message","id":"b2","parentId":"a1","timestamp":"2026-08-27T23:01:41.000Z","message":{"role":"user","content":[{"type":"text","text":"Tidy the README"}],"timestamp":1}}"#;

    #[test]
    fn reads_the_header_and_the_first_prompt() {
        let root = tempfile::tempdir().expect("a temp folder");
        let path = session(root.path(), &[HEADER, MODEL, USER]);
        fs::write(root.path().join("--home-user-proj--/notes.md"), "x").expect("a file");
        let found = sessions(root.path(), &Memos::default());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].agent, Agent::Pi);
        assert_eq!(found[0].id, ID);
        assert_eq!(found[0].cwd, Path::new("/home/user/proj"));
        assert!(found[0].started_at > 0);
        assert_eq!(title(&path).as_deref(), Some("Tidy the README"));
    }
}
