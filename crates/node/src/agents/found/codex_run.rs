//! A Codex subagent's work, read from its rollout (`rollout`, `read`): its
//! role and task from the header, its calls, done once their `*_output` comes,
//! and `task_complete` with what it answered.

use super::newest_files;
use crate::agents::{RunStep, SubagentRun, cut};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

/// The most of a rollout read.
const MAX_READ: u64 = 32 * 1024 * 1024;
/// The most steps kept; all are counted.
const MAX_STEPS: usize = 60;
/// The longest step title.
const MAX_STEP: usize = 120;
/// The longest answer kept.
const MAX_RESULT: usize = 8 * 1024;
/// The arguments a call's title names, the first one it has.
const DETAILS: [&str; 9] = [
    "cmd",
    "command",
    "file_path",
    "path",
    "pattern",
    "query",
    "url",
    "task_name",
    "message",
];

/// The rollout of thread `thread` under the Codex folder `root`.
pub(super) fn rollout(root: &Path, thread: &str) -> Option<PathBuf> {
    if thread.is_empty() || thread.contains(['/', '\\']) {
        return None;
    }
    let suffix = format!("-{thread}.jsonl");
    let named = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
    };
    newest_files(&root.join("sessions"), 3, &named)
        .into_iter()
        .next()
        .map(|(path, _)| path)
}

/// What the rollout at `path` says of the subagent's work.
pub(super) fn read(path: &Path) -> std::io::Result<SubagentRun> {
    let mut reader = Reader::default();
    let file = File::open(path)?;
    let mut skip = 0;
    for (at, line) in BufReader::new(file.take(MAX_READ)).lines().enumerate() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if at == 0 {
            skip = reader.header(&value["payload"]);
        }
        if at >= skip {
            reader.line(&value);
        }
    }
    Ok(reader.finish())
}

#[derive(Debug, Default)]
struct Reader {
    run: SubagentRun,
    /// Each call's step, by its id.
    calls: HashMap<String, usize>,
}

impl Reader {
    /// Takes the header's role and task; returns the line its own work
    /// starts at (`subagent_history_start_ordinal`): a forked subagent's
    /// file starts with its parent's history.
    fn header(&mut self, meta: &Value) -> usize {
        let text = |key: &str| {
            meta[key]
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| cut(text, MAX_STEP))
        };
        self.run.name = text("agent_role").or_else(|| text("agent_nickname"));
        self.run.task = text("agent_path")
            .and_then(|path| path.rsplit('/').next().map(str::to_owned))
            .filter(|task| !task.is_empty() && task != "root");
        meta["subagent_history_start_ordinal"]
            .as_u64()
            .and_then(|at| usize::try_from(at).ok())
            .unwrap_or(0)
    }

    fn line(&mut self, value: &Value) {
        let payload = &value["payload"];
        match (value["type"].as_str(), payload["type"].as_str()) {
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                self.call(payload);
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                let id = payload["call_id"].as_str().unwrap_or_default();
                if let Some(step) = self
                    .calls
                    .get(id)
                    .and_then(|&at| self.run.steps.get_mut(at))
                {
                    step.done = true;
                }
            }
            (Some("response_item"), Some("message")) if payload["role"] == "assistant" => {
                let Value::Array(parts) = &payload["content"] else {
                    return;
                };
                let text: Vec<&str> = parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect();
                let text = text.join("\n");
                if !text.trim().is_empty() {
                    self.run.result = Some(cut(text.trim(), MAX_RESULT));
                }
            }
            (Some("event_msg"), Some("task_started")) => self.run.working = true,
            (Some("event_msg"), Some("task_complete" | "turn_aborted")) => {
                self.run.working = false;
                if let Some(last) = payload["last_agent_message"]
                    .as_str()
                    .map(str::trim)
                    .filter(|last| !last.is_empty())
                {
                    self.run.result = Some(cut(last, MAX_RESULT));
                }
            }
            _ => {}
        }
    }

    fn call(&mut self, payload: &Value) {
        if let Some(id) = payload["call_id"].as_str() {
            self.calls.insert(id.to_owned(), self.run.steps.len());
        }
        self.run.steps.push(RunStep {
            title: call_title(payload).unwrap_or_default(),
            done: false,
        });
    }

    fn finish(mut self) -> SubagentRun {
        self.run.count = self.run.steps.len();
        let extra = self.run.count.saturating_sub(MAX_STEPS);
        self.run.steps.drain(..extra);
        self.run
    }
}

/// The title of the call a `function_call` or `custom_tool_call` payload
/// makes: the tool's name and the first line of what it was given.
pub(super) fn call_title(payload: &Value) -> Option<String> {
    let detail = match payload["type"].as_str()? {
        "function_call" => payload["arguments"]
            .as_str()
            .and_then(|args| serde_json::from_str::<Value>(args).ok())
            .and_then(|args| detail(&args)),
        "custom_tool_call" => payload["input"]
            .as_str()
            .and_then(|input| input.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(str::to_owned),
        _ => return None,
    };
    let name = payload["name"].as_str().unwrap_or("tool");
    let title = match detail {
        Some(detail) if !detail.is_empty() => format!("{name}: {detail}"),
        _ => name.to_owned(),
    };
    Some(cut(&title, MAX_STEP))
}

/// The first line of the first argument a call's title names.
pub(super) fn detail(args: &Value) -> Option<String> {
    DETAILS.iter().find_map(|key| match &args[key] {
        Value::String(text) => text.lines().next().map(|line| line.trim().to_owned()),
        Value::Array(words) => {
            let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
            (!words.is_empty()).then(|| words.join(" "))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    const THREAD: &str = "01a1a004-0000-7000-8000-000000000001";

    /// A fabricated rollout of a forked subagent: two lines of its parent's
    /// history, then its own turn.
    fn rollout_lines(done: bool) -> String {
        let mut lines = vec![
            json!({"type": "session_meta", "payload": {"id": THREAD, "cwd": "/srv/project",
                "source": {"subagent": {"thread_spawn": {"parent_thread_id": "p1", "depth": 1,
                    "agent_path": "/root/sleep_test", "agent_nickname": "Kepler",
                    "agent_role": null}}},
                "agent_nickname": "Kepler", "agent_path": "/root/sleep_test",
                "subagent_history_start_ordinal": 3}}),
            json!({"type": "response_item", "payload": {"type": "function_call",
                "name": "exec_command", "arguments": "{\"cmd\":\"ls\"}", "call_id": "old"}}),
            json!({"type": "response_item", "payload": {"type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": "the parent's answer"}]}}),
            json!({"type": "event_msg", "payload": {"type": "task_started"}}),
            json!({"type": "response_item", "payload": {"type": "function_call",
                "name": "sleep", "arguments": "{\"duration_ms\":60000}", "call_id": "c1"}}),
            json!({"type": "response_item", "payload": {"type": "function_call_output",
                "call_id": "c1", "output": "Sleep completed."}}),
            json!({"type": "response_item", "payload": {"type": "custom_tool_call",
                "name": "exec", "input": "\nrun('cargo test')\n", "call_id": "c2"}}),
        ];
        if done {
            lines.push(
                json!({"type": "event_msg", "payload": {"type": "task_complete",
                "last_agent_message": "Slept for a minute."}}),
            );
        }
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn write(root: &Path, done: bool) -> PathBuf {
        let dir = root.join("sessions/2026/11/03");
        fs::create_dir_all(&dir).expect("folders");
        let path = dir.join(format!("rollout-2026-11-03T07-07-01-{THREAD}.jsonl"));
        fs::write(&path, rollout_lines(done)).expect("a rollout");
        path
    }

    #[test]
    fn a_subagents_rollout_is_found_by_its_thread() {
        let root = tempfile::tempdir().expect("a temp folder");
        let path = write(root.path(), true);
        assert_eq!(rollout(root.path(), THREAD), Some(path));
        assert_eq!(rollout(root.path(), "other"), None);
        assert_eq!(rollout(root.path(), "../x"), None);
    }

    #[test]
    fn its_own_steps_and_answer_read_after_the_parents_history() {
        let root = tempfile::tempdir().expect("a temp folder");
        let run = read(&write(root.path(), true)).expect("the rollout");
        assert_eq!(
            run,
            SubagentRun {
                steps: vec![
                    RunStep {
                        title: "sleep".into(),
                        done: true
                    },
                    RunStep {
                        title: "exec: run('cargo test')".into(),
                        done: false
                    },
                ],
                count: 2,
                result: Some("Slept for a minute.".into()),
                working: false,
                name: Some("Kepler".into()),
                task: Some("sleep_test".into()),
            }
        );
        let running = read(&write(root.path(), false)).expect("the rollout");
        assert!(running.working);
        assert_eq!(running.result, None);
    }
}
