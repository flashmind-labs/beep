//! The subagents a Claude Code session started (`attach`), read from
//! `<session>/subagents/agent-<id>.jsonl` and its `.meta.json`, each shown on
//! the entry of the call that started it.

use super::transcript::{Spawn, read_tail, tool_line};
use crate::agents::cut;
use beep_proto::node::agent::Entry;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// The most subagent files one session's folder is read for.
const MAX_AGENTS: usize = 1_000;
/// The longest meta file read.
const MAX_META: u64 = 64 * 1024;
/// How much of the end of a subagent's file is read.
const MAX_READ: u64 = 4 * 1024 * 1024;
/// The most steps an entry names; it counts them all.
const MAX_STEPS: usize = 30;
/// The longest step line.
const MAX_STEP: usize = 120;
/// The longest subagent entry.
const MAX_ENTRY: usize = 12 * 1024;
/// A subagent not marked finished whose file changed this recently still
/// runs.
const RUNNING: Duration = Duration::from_secs(2 * 60);

/// What `agent-<id>.meta.json` says.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Meta {
    agent_type: Option<String>,
    /// A name other than its type, such as the skill that started it.
    name: Option<String>,
    description: Option<String>,
    tool_use_id: Option<String>,
    finished: Option<bool>,
    stopped_by_user: Option<bool>,
}

/// One subagent's files.
#[derive(Debug)]
struct Found {
    /// The id its files are named by.
    id: String,
    transcript: PathBuf,
    meta: Meta,
}

/// How a subagent or one of its steps stands, by its wire name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    InProgress,
    Completed,
    Failed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// A subagent's own lines: the calls it made and its last text.
#[derive(Debug, Default)]
struct Run {
    steps: Vec<Step>,
    /// Each call's step, by its id.
    calls: HashMap<String, usize>,
    result: Option<String>,
}

#[derive(Debug)]
struct Step {
    status: Status,
    title: String,
}

/// Writes the subagent each of `spawns` started on its entry among
/// `entries`, which start at the digest's entry `skip`. A call whose
/// subagent's file is not found stays as it was.
pub(super) fn attach(
    session: &Path,
    spawns: &HashMap<String, Spawn>,
    entries: &mut [Entry],
    skip: usize,
) {
    let mut shown: Vec<(&String, &Spawn)> = spawns
        .iter()
        .filter(|(_, spawn)| spawn.entry >= skip)
        .collect();
    if shown.is_empty() {
        return;
    }
    shown.sort_by_key(|(_, spawn)| spawn.entry);
    let found = agents(&session.with_extension("").join("subagents"));
    let mut taken = HashSet::new();
    for (call, spawn) in shown {
        let Some(agent) = pick(&found, call, spawn, &taken) else {
            continue;
        };
        taken.insert(agent.id.as_str());
        let Some(entry) = entries.get_mut(spawn.entry - skip) else {
            continue;
        };
        entry.text = text(spawn, agent, &read(&agent.transcript));
    }
}

/// The subagent `call` started: the one whose meta names the call, else
/// the one its result named, else one not taken with its description.
fn pick<'a>(
    found: &'a [Found],
    call: &str,
    spawn: &Spawn,
    taken: &HashSet<&str>,
) -> Option<&'a Found> {
    let free = |agent: &&Found| !taken.contains(agent.id.as_str());
    found
        .iter()
        .find(|agent| agent.meta.tool_use_id.as_deref() == Some(call))
        .or_else(|| {
            let id = spawn.agent_id.as_deref()?;
            found.iter().find(|agent| agent.id == id)
        })
        .or_else(|| {
            let description = spawn.description.as_deref()?;
            found.iter().filter(free).find(|agent| {
                agent.meta.tool_use_id.is_none()
                    && agent.meta.description.as_deref().map(str::trim) == Some(description)
            })
        })
}

/// The subagents in `dir`, each with its meta when it has one.
fn agents(dir: &Path) -> Vec<Found> {
    let Ok(files) = fs::read_dir(dir) else {
        return Vec::new();
    };
    files
        .flatten()
        .filter_map(|file| {
            let name = file.file_name();
            let id = name
                .to_str()?
                .strip_suffix(".jsonl")?
                .strip_prefix("agent-")?;
            Some(Found {
                id: id.to_owned(),
                transcript: file.path(),
                meta: meta(&dir.join(format!("agent-{id}.meta.json"))),
            })
        })
        .take(MAX_AGENTS)
        .collect()
}

/// The meta at `path`; empty when there is none or it does not read.
fn meta(path: &Path) -> Meta {
    let Ok(file) = fs::File::open(path) else {
        return Meta::default();
    };
    let reader = std::io::Read::take(file, MAX_META);
    match serde_json::from_reader(reader) {
        Ok(meta) => meta,
        Err(e) => {
            log::debug!("subagent meta not read; path={} error={e}", path.display());
            Meta::default()
        }
    }
}

/// The end of the subagent's file at `path`: the calls it made and what
/// it said last. Empty when it does not read.
fn read(path: &Path) -> Run {
    let mut run = Run::default();
    let tail = match read_tail(path, MAX_READ) {
        Ok(tail) => tail,
        Err(e) => {
            log::debug!("subagent not read; path={} error={e}", path.display());
            return run;
        }
    };
    for line in tail.text.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
            run.line(&value);
        }
    }
    run
}

impl Run {
    fn line(&mut self, value: &Value) {
        let Value::Array(blocks) = &value["message"]["content"] else {
            return;
        };
        match value["type"].as_str() {
            Some("assistant") => {
                for block in blocks {
                    match block["type"].as_str() {
                        Some("text") => {
                            let text = block["text"].as_str().unwrap_or("").trim();
                            if !text.is_empty() {
                                self.result = Some(text.to_owned());
                            }
                        }
                        Some("tool_use") => {
                            if let Some(id) = block["id"].as_str() {
                                self.calls.insert(id.to_owned(), self.steps.len());
                            }
                            self.steps.push(Step {
                                status: Status::InProgress,
                                title: tool_line(block),
                            });
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for block in blocks.iter().filter(|block| block["type"] == "tool_result") {
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    let Some(step) = self.calls.get(id).and_then(|&at| self.steps.get_mut(at))
                    else {
                        continue;
                    };
                    step.status = if block["is_error"].as_bool() == Some(true) {
                        Status::Failed
                    } else {
                        Status::Completed
                    };
                }
            }
            _ => {}
        }
    }
}

/// How the subagent stands: failed when its call failed or the user
/// stopped it, done when it says it finished or its file has not changed
/// lately, else still working.
fn status(spawn: &Spawn, agent: &Found) -> Status {
    if spawn.failed || agent.meta.stopped_by_user == Some(true) {
        return Status::Failed;
    }
    if agent.meta.finished == Some(true) {
        return Status::Completed;
    }
    let changed = fs::metadata(&agent.transcript).and_then(|meta| meta.modified());
    let recent = changed
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age < RUNNING);
    if recent {
        Status::InProgress
    } else {
        Status::Completed
    }
}

/// The entry of the call that started `agent`, as a live session writes it.
fn text(spawn: &Spawn, agent: &Found, run: &Run) -> String {
    let status = status(spawn, agent);
    let title = spawn
        .description
        .as_deref()
        .or(agent.meta.description.as_deref())
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Subagent");
    let title = title.lines().next().unwrap_or_default();
    let mut text = format!("{title} [{}]\n[steps] {}", status.as_str(), run.steps.len());
    for step in &run.steps[run.steps.len().saturating_sub(MAX_STEPS)..] {
        // A step left running in a finished subagent did not fail.
        let shown = match step.status {
            Status::InProgress if status != Status::InProgress => Status::Completed,
            other => other,
        };
        let title = step.title.lines().next().unwrap_or_default();
        text.push_str(&format!(
            "\n[step {}] {}",
            shown.as_str(),
            cut(title, MAX_STEP)
        ));
    }
    let name = agent
        .meta
        .name
        .as_deref()
        .or(spawn.agent.as_deref())
        .or(agent.meta.agent_type.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(name) = name {
        text.push_str(&format!("\n[agent] {}", cut(name, MAX_STEP)));
    }
    if let Some(result) = &run.result {
        text.push('\n');
        text.push_str(result);
    }
    cut(&text, MAX_ENTRY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{claude::transcript::entries, found::memo::Memo};
    use beep_proto::node::agent::EntryKind;
    use serde_json::json;

    /// A fabricated session that starts two subagents, an `Explore` one
    /// and a custom `release-checker`, with their files beside it.
    fn session(root: &Path) -> PathBuf {
        let project = root.join("projects/-srv-project");
        let subagents = project.join("s1/subagents");
        fs::create_dir_all(&subagents).expect("folders");
        let lines = [
            json!({"type": "user", "message": {"role": "user", "content": "check the release"}}),
            json!({"type": "assistant", "message": {"content": [
                {"type": "tool_use", "id": "call_explore", "name": "Agent", "input": {
                    "description": "Find the config loader", "subagent_type": "Explore",
                    "prompt": "Where is the config read?"}},
                {"type": "tool_use", "id": "call_release", "name": "Task", "input": {
                    "description": "Check the release notes",
                    "subagent_type": "release-checker", "prompt": "Compare the notes"}}
            ]}}),
            json!({"type": "user", "toolUseResult": {"agentId": "a2", "status": "completed"},
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "call_release", "content": "done"}
            ]}}),
            json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_explore", "content": "found"}
            ]}}),
            json!({"type": "assistant", "message": {"content": [
                {"type": "text", "text": "Both checks are done."}
            ]}}),
        ];
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        let path = project.join("s1.jsonl");
        fs::write(&path, text).expect("the session");

        let explore = [
            json!({"type": "user", "isSidechain": true, "message": {"role": "user",
                "content": "Where is the config read?"}}),
            json!({"type": "assistant", "isSidechain": true, "message": {"content": [
                {"type": "tool_use", "id": "t1", "name": "Grep", "input": {"pattern": "load_config"}}
            ]}}),
            json!({"type": "user", "isSidechain": true, "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "src/config.rs"}
            ]}}),
            json!({"type": "assistant", "isSidechain": true, "message": {"content": [
                {"type": "tool_use", "id": "t2", "name": "Read",
                    "input": {"file_path": "/srv/project/src/config.rs"}}
            ]}}),
            json!({"type": "user", "isSidechain": true, "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t2", "is_error": true, "content": "no"}
            ]}}),
            json!({"type": "assistant", "isSidechain": true, "message": {"content": [
                {"type": "text", "text": "It is read in src/config.rs."}
            ]}}),
        ];
        let explore: String = explore.iter().map(|line| format!("{line}\n")).collect();
        fs::write(subagents.join("agent-a1.jsonl"), explore).expect("a subagent");
        let meta = json!({"agentType": "Explore", "description": "Find the config loader",
            "toolUseId": "call_explore", "finished": true});
        fs::write(subagents.join("agent-a1.meta.json"), meta.to_string()).expect("its meta");

        // An older subagent: no meta, linked by the id its result named.
        let release = json!({"type": "assistant", "isSidechain": true, "message": {"content": [
            {"type": "text", "text": "The notes match the diff."}
        ]}});
        fs::write(subagents.join("agent-a2.jsonl"), format!("{release}\n")).expect("a subagent");
        path
    }

    #[test]
    fn each_subagent_goes_on_the_call_that_started_it() {
        let root = tempfile::tempdir().expect("a temp folder");
        let path = session(root.path());
        let read = entries(&path, 10, None, &Memo::new(2)).expect("the transcript");
        let tools: Vec<&str> = read
            .entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::Tool)
            .map(|entry| entry.text.as_str())
            .collect();
        // The release check has no meta saying it finished, and its file
        // was just written, so it reads as working.
        assert_eq!(
            tools[0],
            "Find the config loader [completed]\n[steps] 2\n\
             [step completed] Grep: load_config\n\
             [step failed] Read: /srv/project/src/config.rs\n\
             [agent] Explore\n\
             It is read in src/config.rs."
        );
        assert_eq!(
            tools[1],
            "Check the release notes [in_progress]\n[steps] 0\n\
             [agent] release-checker\n\
             The notes match the diff."
        );
    }

    #[test]
    fn a_subagent_before_the_entries_read_is_not_read() {
        let root = tempfile::tempdir().expect("a temp folder");
        let path = session(root.path());
        let read = entries(&path, 1, None, &Memo::new(2)).expect("the transcript");
        assert_eq!(read.entries.len(), 1);
        assert_eq!(read.entries[0].text, "Both checks are done.");
    }
}
