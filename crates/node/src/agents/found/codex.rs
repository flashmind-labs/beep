//! Codex's sessions (`sessions`, `title`):
//! `<CODEX_HOME>/sessions/<yyyy>/<mm>/<dd>/rollout-*.jsonl`, with the titles
//! the user gave in `<CODEX_HOME>/session_index.jsonl`.

use super::{Agent, Memos, Seen, first_line, head_lines, newest_files, prompt_title};
use crate::agents::claude::unix_seconds;
use serde_json::Value;
use std::{
    collections::HashMap,
    convert::Infallible,
    env,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

/// The longest title index read.
const MAX_INDEX: u64 = 8 * 1024 * 1024;

/// Codex's folder, when it has sessions here.
pub(super) fn locate() -> Option<PathBuf> {
    let root = match env::var_os("CODEX_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?.join(".codex"),
    };
    root.join("sessions").is_dir().then_some(root)
}

/// The newest sessions under `root`, sub-agents left out.
pub(super) fn sessions(root: &Path, memos: &Memos) -> Vec<Seen> {
    let rollout = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
    };
    let index = root.join("session_index.jsonl");
    let Ok(names) = memos
        .names
        .read(&index, || Ok::<_, Infallible>(Arc::new(titles(&index))));
    newest_files(&root.join("sessions"), 3, &rollout)
        .into_iter()
        .filter_map(|(path, at)| {
            let read = || first_line(&path).map(|line| line.as_deref().and_then(header));
            let mut seen = memos.heads.read(&path, read).ok()??;
            seen.last_activity = at;
            seen.title = names.get(&seen.id).cloned();
            seen.file = Some(path);
            Some(seen)
        })
        .collect()
}

/// The session a rollout's first line (`session_meta`) names; `None` for a
/// sub-agent's (its `source` is an object) or a line that is not a header.
fn header(line: &str) -> Option<Seen> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    if value["type"] != "session_meta" {
        return None;
    }
    let meta = &value["payload"];
    if meta["source"].is_object() {
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
        id: id.to_owned(),
        cwd: PathBuf::from(cwd),
        started_at: started,
        last_activity: started,
        title: None,
        file: None,
    })
}

/// The names the user gave sessions, by id, from the index at `index`; a
/// later line wins.
fn titles(index: &Path) -> HashMap<String, String> {
    let Ok(file) = File::open(index) else {
        return HashMap::new();
    };
    let mut names = HashMap::new();
    for line in BufReader::new(file.take(MAX_INDEX))
        .lines()
        .map_while(Result::ok)
    {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let (Some(id), Some(name)) = (value["id"].as_str(), value["thread_name"].as_str())
            && let Some(name) = prompt_title(name)
        {
            names.insert(id.to_owned(), name);
        }
    }
    names
}

/// The first prompt in the rollout at `path`.
pub(super) fn title(path: &Path) -> Option<String> {
    head_lines(path).ok()?.find_map(|line| prompt(&line))
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
            let text: Vec<&str> = payload["item"]["content"]
                .as_array()?
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect();
            prompt_title(&text.join("\n"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ID: &str = "01a199e3-03f2-7820-a29e-067ae8c4ec85";

    fn write(root: &Path, name: &str, lines: &[String]) -> PathBuf {
        let dir = root.join("sessions/2026/11/02");
        fs::create_dir_all(&dir).expect("folders");
        let path = dir.join(name);
        fs::write(&path, lines.join("\n") + "\n").expect("a rollout");
        path
    }

    fn meta(id: &str, source: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-11-02T01:32:34.000Z","type":"session_meta","payload":{{"id":"{id}","timestamp":"2026-11-02T01:32:34.000Z","cwd":"/home/user/proj","source":{source},"base_instructions":{{"text":"You are Codex."}}}}}}"#
        )
    }

    #[test]
    fn reads_rollouts_and_leaves_out_sub_agents() {
        let root = tempfile::tempdir().expect("a temp folder");
        let user = r#"{"type":"event_msg","payload":{"type":"user_message","message":"Add a retry flag"}}"#;
        let context = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>"}]}}"#;
        let path = write(
            root.path(),
            &format!("rollout-2026-11-02T02-32-34-{ID}.jsonl"),
            &[meta(ID, r#""cli""#), context.into(), user.into()],
        );
        write(
            root.path(),
            "rollout-2026-11-02T02-40-00-sub.jsonl",
            &[meta("sub", r#"{"subagent":"review"}"#)],
        );
        write(root.path(), "notes.jsonl", &[meta("x", r#""cli""#)]);
        let found = sessions(root.path(), &Memos::default());
        assert_eq!(found.len(), 1, "{found:?}");
        let one = &found[0];
        assert_eq!(one.agent, Agent::Codex);
        assert_eq!(one.id, ID);
        assert_eq!(one.cwd, Path::new("/home/user/proj"));
        assert!(one.started_at > 0);
        assert_eq!(one.title, None);
        assert_eq!(title(&path).as_deref(), Some("Add a retry flag"));
    }

    #[test]
    fn a_newer_user_message_reads_too() {
        let line = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"i","content":[{"type":"text","text":"Fix the parser","text_elements":[]}]}}}"#;
        assert_eq!(prompt(line).as_deref(), Some("Fix the parser"));
        assert_eq!(header("{\"type\":\"turn_context\"}"), None);
    }
}
