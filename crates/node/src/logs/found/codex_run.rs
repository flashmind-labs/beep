//! A Codex subagent's work, read from its rollout (`rollout`, `read`): its
//! role and task from the header, its calls, done once their `*_output` comes,
//! and `task_complete` with what it answered.

use super::common::{Child, newest_files};
use crate::logs::subagent::Conversation;
use crate::logs::subagent::Run as SubagentRun;
use crate::logs::subagent::RunStep;
use crate::logs::window::{content_text, titled_call};
use crate::text::cut;
use beep_proto::SessionId;
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

/// Parent and native conversation ID from a child rollout's header.
pub(crate) fn parent(line: &str) -> Option<Child> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value["type"] != "session_meta" {
        return None;
    }
    let meta = &value["payload"];
    let parent = meta["source"]["subagent"]["thread_spawn"]["parent_thread_id"].as_str()?;
    let thread = meta["id"].as_str()?;
    let session = Conversation::Codex(thread).to_string();
    Conversation::try_from(session.as_str()).ok()?;
    (parent != thread).then(|| Child {
        parent: Into::<SessionId>::into(parent),
        session: Into::<SessionId>::into(session),
    })
}

/// The rollout of thread `thread` under the Codex folder `root`.
pub(crate) fn rollout(root: &Path, thread: &str) -> Option<PathBuf> {
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
pub(crate) fn read(path: &Path) -> std::io::Result<SubagentRun> {
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
        self.run.session = meta["id"].as_str().and_then(|id| {
            let session = Conversation::Codex(id).to_string();
            Conversation::try_from(session.as_str())
                .is_ok()
                .then_some(Into::<SessionId>::into(session))
        });
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
            (Some("turn_context"), _) => {
                if let Some(model) = payload["model"].as_str().map(str::trim)
                    && !model.is_empty()
                {
                    self.run.model = Some(cut(model, MAX_STEP));
                }
            }
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
                if payload["content"].is_array()
                    && let Some(text) = content_text(&payload["content"])
                    && !text.trim().is_empty()
                {
                    self.run.result = Some(cut(text.trim(), MAX_RESULT));
                }
            }
            (Some("event_msg"), Some("task_started")) => self.run.working = true,
            (Some("event_msg"), Some("task_complete" | "turn_aborted")) => {
                self.run.working = false;

                if let Some(last) = payload["last_agent_message"].as_str().map(str::trim)
                    && !last.is_empty()
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
pub(crate) fn call_title(payload: &Value) -> Option<String> {
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
    Some(cut(&titled_call(name, detail.as_deref()), MAX_STEP))
}

/// The first line of the first argument a call's title names.
pub(crate) fn detail(args: &Value) -> Option<String> {
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
#[path = "../../tests/logs_found_codex_run.rs"]
mod tests;
