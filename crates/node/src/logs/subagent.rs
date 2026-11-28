//! Subagents in a live session (`Subagent`, `Activity`, `Run`): the tool
//! call that started one, and what its entry says of it (written in
//! `super::live`).

use crate::text::valid_id;
use anyhow::{Context, Result, bail};
use beep_proto::SessionId;
use beep_proto::node::agent::{Entry, EntryKind};
use serde_json::Value;

/// A native conversation, addressed separately from the process that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Conversation<'a> {
    Codex(&'a str),
    Agy(&'a str),
    Claude { parent: &'a str, agent: &'a str },
}

impl<'a> TryFrom<&'a str> for Conversation<'a> {
    type Error = anyhow::Error;

    fn try_from(id: &'a str) -> Result<Self> {
        let mut parts = id
            .strip_prefix("subagent:")
            .context("missing subagent prefix")?
            .split(':');
        let harness = parts.next().context("missing subagent harness")?;
        let first = parts
            .next()
            .filter(|part| valid_id(part))
            .context("invalid subagent id")?;
        let found = match harness {
            "codex" => Self::Codex(first),
            "agy" => Self::Agy(first),
            "claude" => Self::Claude {
                parent: first,
                agent: parts
                    .next()
                    .filter(|part| valid_id(part))
                    .context("invalid Claude agent id")?,
            },
            _ => bail!("unknown subagent harness"),
        };

        if parts.next().is_some() {
            bail!("too many subagent id parts");
        }

        Ok(found)
    }
}

impl std::fmt::Display for Conversation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Codex(thread) => write!(f, "subagent:codex:{thread}"),
            Self::Agy(child) => write!(f, "subagent:agy:{child}"),
            Self::Claude { parent, agent } => write!(f, "subagent:claude:{parent}:{agent}"),
        }
    }
}

/// The title codex-acp gives the call that starts subagents.
const CODEX_SPAWN: &str = "spawnAgent";
/// The tool pi's subagent packages add, which also manages their runs.
const PI_SPAWN: &str = "subagent";
/// Claude Code's tools that start a subagent, the older name last.
pub(crate) const CLAUDE_SPAWNS: [&str; 2] = ["Agent", "Task"];
/// The longest name or title taken from a call.
const MAX_NAME: usize = 120;

/// What a tool call that started a subagent says of it.
#[derive(Debug, Default, Clone)]
pub(crate) struct Subagent {
    /// Its type or custom agent's name, such as `Explore` (Claude Code's
    /// `subagent_type`).
    pub(crate) name: Option<String>,
    /// The model requested for this subagent.
    pub(crate) model: Option<String>,
    /// The Codex threads it runs in.
    pub(crate) threads: Vec<String>,
    /// Codex said it ended.
    pub(crate) ended: bool,
    /// Started by Codex, which sends none of its calls.
    pub(crate) codex: bool,
    /// The title its entry shows in place of the call's: a Codex
    /// subagent's task, else the first line of what it was asked.
    pub(crate) title: Option<String>,
}

impl Subagent {
    /// The subagent the call `title` of tool `tool` with `input` starts, if
    /// it starts one.
    pub(crate) fn of_call(tool: Option<&str>, title: &str, input: Option<&Value>) -> Option<Self> {
        let input = input?;

        if tool == Some("flsh_agent") && input["flshAgent"] == true {
            return Some(Self {
                name: text(input, "name"),
                model: text(input, "model"),
                ..Self::default()
            });
        }

        if tool == Some("delegate") || (tool.is_none() && title == "delegate") {
            return Some(Self {
                name: text(input, "name").or_else(|| text(input, "persona")),
                model: text(input, "model"),
                title: text(input, "task"),
                ..Self::default()
            });
        }

        if tool.unwrap_or(title) == PI_SPAWN {
            return pi_spawn(input);
        }

        if title == CODEX_SPAWN {
            let asked = input["prompt"]
                .as_str()
                .and_then(|prompt| prompt.lines().next());
            return Some(Self {
                threads: threads(input),
                model: text(input, "model"),
                codex: true,
                title: Some(asked.map_or_else(
                    || "Subagent".to_owned(),
                    |line| crate::text::cut(line.trim(), MAX_NAME),
                )),
                ..Self::default()
            });
        }

        if let Ok(activity) = Activity::try_from(input) {
            return Some(Self {
                threads: vec![activity.thread],
                ended: activity.ended,
                codex: true,
                title: Some(activity.task),
                ..Self::default()
            });
        }

        // Claude Code leaves out `subagent_type` for its default agent.
        let name = text(input, "subagent_type");
        if name.is_none() && !tool.is_some_and(|tool| CLAUDE_SPAWNS.contains(&tool)) {
            return None;
        }

        Some(Self {
            name,
            model: text(input, "model"),
            ..Self::default()
        })
    }

    /// Takes the threads a later update of a Codex spawn names.
    pub(crate) fn update(&mut self, input: Option<&Value>) {
        for thread in input.map(threads).unwrap_or_default() {
            if !self.threads.contains(&thread) {
                self.threads.push(thread);
            }
        }
    }
}

/// A pi `subagent` call that runs one agent (`agent`, `task`), several in
/// parallel (`tasks`) or a chain (`chain`). A call with an `action` manages
/// runs and starts none.
fn pi_spawn(input: &Value) -> Option<Subagent> {
    if input.get("action").is_some() {
        return None;
    }

    let first = ["tasks", "chain"]
        .iter()
        .find_map(|key| input[*key].as_array()?.first())
        .unwrap_or(input);
    let name = text(first, "agent")?;
    let title = first["task"]
        .as_str()
        .and_then(|task| task.lines().map(str::trim).find(|line| !line.is_empty()))
        .map(|line| crate::text::cut(line, MAX_NAME));

    Some(Subagent {
        name: Some(name),
        model: text(first, "model").or_else(|| text(input, "model")),
        title,
        ..Subagent::default()
    })
}

/// codex-acp's report that a subagent started, ended or was talked to
/// (`agentThreadId`, `agentPath`). codex-acp sends none of a subagent's own
/// calls, only `spawnAgent` with the threads it started
/// (`receiverThreadIds`), so its steps are read from the thread's rollout
/// (`crate::logs::found::Stores::codex_run`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Activity {
    pub(crate) thread: String,
    /// Its task's name, from the last part of its path: `sleep_test`.
    pub(crate) task: String,
    pub(crate) ended: bool,
}

impl TryFrom<&Value> for Activity {
    type Error = anyhow::Error;

    /// The activity `input` reports; rejects any other call, and
    /// the root agent's own.
    fn try_from(input: &Value) -> Result<Self> {
        let thread = text(input, "agentThreadId").context("missing agent thread id")?;
        let path = input["agentPath"].as_str().context("missing agent path")?;
        let task = path
            .trim()
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .map(str::trim)
            .filter(|task| !task.is_empty() && *task != "root")
            .map(|task| crate::text::cut(task, MAX_NAME))
            .context("not a subagent path")?;
        let ended = matches!(
            input["activityKind"].as_str(),
            Some("completed" | "interrupted")
        );
        Ok(Self {
            thread,
            task,
            ended,
        })
    }
}

/// What a subagent's own log says, read from disk.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Run {
    /// The independently readable conversation, when its file exists.
    pub(crate) session: Option<SessionId>,
    /// Its last steps, oldest first.
    pub(crate) steps: Vec<RunStep>,
    /// How many steps it ran in all.
    pub(crate) count: usize,
    /// What it said last.
    pub(crate) result: Option<String>,
    /// It has not finished its work.
    pub(crate) working: bool,
    /// Its role or name.
    pub(crate) name: Option<String>,
    /// The model recorded in its own transcript.
    pub(crate) model: Option<String>,
    /// Its task's name.
    pub(crate) task: Option<String>,
}

impl Run {
    /// A recovered child uses the same card format as an ACP spawn.
    pub(crate) fn entry(&self) -> Entry {
        let status = if self.working {
            "in_progress"
        } else {
            "completed"
        };
        self.entry_with_status(status)
    }

    pub(crate) fn entry_with_status(&self, status: &str) -> Entry {
        let title = self
            .task
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or("Subagent");
        let mut text = format!("{title} [{status}]\n[steps] {}", self.count);
        for step in &self.steps {
            let status = if step.done || !self.working {
                "completed"
            } else {
                "in_progress"
            };
            text.push_str(&format!("\n[step {status}] {}", step.title));
        }
        if let Some(name) = &self.name {
            text.push_str(&format!("\n[agent] {name}"));
        }
        if let Some(session) = &self.session {
            text.push_str(&format!("\n[session] {session}"));
        }
        if let Some(model) = &self.model {
            text.push_str(&format!("\n[model] {model}"));
        }
        if let Some(result) = &self.result {
            text.push('\n');
            text.push_str(result);
        }
        Entry::new(EntryKind::Tool, crate::text::cut(&text, 16 * 1024))
    }
}

/// One call a subagent made.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RunStep {
    pub(crate) title: String,
    pub(crate) done: bool,
}

/// The threads in a Codex spawn's `receiverThreadIds`.
fn threads(input: &Value) -> Vec<String> {
    let Value::Array(ids) = &input["receiverThreadIds"] else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

fn text(input: &Value, key: &str) -> Option<String> {
    input[key]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| crate::text::cut(text, MAX_NAME))
}

#[cfg(test)]
#[path = "../tests/logs_subagent.rs"]
mod tests;
