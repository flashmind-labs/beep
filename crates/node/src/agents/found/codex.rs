//! Codex's sessions (`sessions`, `title`):
//! `<CODEX_HOME>/sessions/<yyyy>/<mm>/<dd>/rollout-*.jsonl`, with the titles
//! the user gave in `<CODEX_HOME>/session_index.jsonl`.

use super::{Agent, Memos, Seen, first_line, head_lines, newest_files, prompt_title};
use crate::agents::claude::unix_seconds;
use crate::agents::place::call_folder;
use serde_json::Value;
use std::{
    collections::HashMap,
    convert::Infallible,
    env,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

/// The longest title index read.
const MAX_INDEX: u64 = 8 * 1024 * 1024;
/// The most of a rollout's end read for the folder it works in.
const MAX_TAIL: u64 = 2 * 1024 * 1024;

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

/// The folder the rollout at `path` last worked in: the `cwd` of its last
/// `turn_context`, or the `workdir` a later tool call gave (taken from the
/// folder before it when relative), read from the end of the file. `None`
/// when that stretch names none; the header's `cwd` then holds.
pub(super) fn latest_folder(path: &Path) -> Option<PathBuf> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let cut_at = len.saturating_sub(MAX_TAIL);
    file.seek(SeekFrom::Start(cut_at)).ok()?;
    let mut reader = BufReader::new(file.take(MAX_TAIL));
    if cut_at > 0 {
        reader.read_until(b'\n', &mut Vec::new()).ok()?;
    }
    let mut turn: Option<PathBuf> = None;
    let mut folder: Option<PathBuf> = None;
    for line in reader.lines().map_while(Result::ok) {
        let is_turn = line.contains("\"turn_context\"");
        if !is_turn && !line.contains("workdir") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = &value["payload"];
        if is_turn && value["type"] == "turn_context" {
            if let Some(cwd) = payload["cwd"].as_str().filter(|cwd| !cwd.is_empty()) {
                turn = Some(PathBuf::from(cwd));
                folder.clone_from(&turn);
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
    fn the_latest_folder_is_a_calls_workdir_after_the_turns_cwd() {
        let root = tempfile::tempdir().expect("a temp folder");
        let turn = |cwd: &str| {
            format!(r#"{{"type":"turn_context","payload":{{"cwd":"{cwd}","model":"m"}}}}"#)
        };
        let call = |args: &str| {
            let args = args.replace('"', "\\\"");
            format!(
                r#"{{"type":"response_item","payload":{{"type":"function_call","name":"exec_command","arguments":"{args}","call_id":"c"}}}}"#
            )
        };
        let path = write(
            root.path(),
            "rollout-a.jsonl",
            &[
                meta(ID, r#""cli""#),
                turn("/srv/project"),
                call(r#"{"cmd":"ls","workdir":"/srv/project/.worktrees/fix"}"#),
            ],
        );
        assert_eq!(
            latest_folder(&path),
            Some(PathBuf::from("/srv/project/.worktrees/fix"))
        );
        // A later turn's cwd wins over an earlier call's workdir.
        let path = write(
            root.path(),
            "rollout-b.jsonl",
            &[
                turn("/srv/project"),
                call(r#"{"cmd":"ls","workdir":"/srv/project/.worktrees/fix"}"#),
                turn("/srv/project/.worktrees/other"),
            ],
        );
        assert_eq!(
            latest_folder(&path),
            Some(PathBuf::from("/srv/project/.worktrees/other"))
        );
        let none = write(root.path(), "rollout-c.jsonl", &[meta(ID, r#""cli""#)]);
        assert_eq!(latest_folder(&none), None);
    }

    #[test]
    fn a_newer_user_message_reads_too() {
        let line = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"i","content":[{"type":"text","text":"Fix the parser","text_elements":[]}]}}}"#;
        assert_eq!(prompt(line).as_deref(), Some("Fix the parser"));
        assert_eq!(header("{\"type\":\"turn_context\"}"), None);
    }
}
