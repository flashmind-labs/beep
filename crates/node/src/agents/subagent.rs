//! Subagents in a live session (`Subagent`, `Activity`, `Run`): the tool
//! call that started one, and what its entry says of it (written in
//! `super::live`).

use serde_json::Value;

/// The title codex-acp gives the call that starts subagents.
const CODEX_SPAWN: &str = "spawnAgent";
/// The longest name or title taken from a call.
const MAX_NAME: usize = 120;

/// What a tool call that started a subagent says of it.
#[derive(Debug, Default, Clone)]
pub(crate) struct Subagent {
    /// Its type or custom agent's name, such as `Explore` (Claude Code's
    /// `subagent_type`).
    pub(crate) name: Option<String>,
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
    /// The subagent the call `title` with `input` starts, if it starts one.
    pub(crate) fn of_call(title: &str, input: Option<&Value>) -> Option<Self> {
        let input = input?;
        if title == CODEX_SPAWN {
            let asked = input["prompt"]
                .as_str()
                .and_then(|prompt| prompt.lines().next());
            return Some(Self {
                threads: threads(input),
                codex: true,
                title: Some(asked.map_or_else(
                    || "Subagent".to_owned(),
                    |line| crate::agents::cut(line.trim(), MAX_NAME),
                )),
                ..Self::default()
            });
        }
        if let Some(activity) = Activity::of(input) {
            return Some(Self {
                threads: vec![activity.thread],
                ended: activity.ended,
                codex: true,
                title: Some(activity.task),
                ..Self::default()
            });
        }
        let name = text(input, "subagent_type")?;
        Some(Self {
            name: Some(name),
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

/// codex-acp's report that a subagent started, ended or was talked to
/// (`agentThreadId`, `agentPath`). codex-acp sends none of a subagent's own
/// calls, only `spawnAgent` with the threads it started
/// (`receiverThreadIds`), so its steps are read from the thread's rollout
/// (`crate::agents::found::Stores::codex_run`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Activity {
    pub(crate) thread: String,
    /// Its task's name, from the last part of its path: `sleep_test`.
    pub(crate) task: String,
    pub(crate) ended: bool,
}

impl Activity {
    /// The activity `input` reports; `None` for any other call, and for
    /// the root agent's own.
    pub(crate) fn of(input: &Value) -> Option<Self> {
        let thread = text(input, "agentThreadId")?;
        let path = input["agentPath"].as_str()?;
        let task = path
            .trim()
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .map(str::trim)
            .filter(|task| !task.is_empty() && *task != "root")
            .map(|task| crate::agents::cut(task, MAX_NAME))?;
        let ended = matches!(
            input["activityKind"].as_str(),
            Some("completed" | "interrupted")
        );
        Some(Self {
            thread,
            task,
            ended,
        })
    }
}

/// What a subagent's own log says, read from disk.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Run {
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
    /// Its task's name.
    pub(crate) task: Option<String>,
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
        .map(|text| crate::agents::cut(text, MAX_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_codex_spawn_names_its_threads_once_it_has_them() {
        let started = json!({"prompt": "sleep", "receiverThreadIds": []});
        let mut found = Subagent::of_call(CODEX_SPAWN, Some(&started)).expect("a subagent");
        assert!(found.threads.is_empty());
        assert_eq!(found.title.as_deref(), Some("sleep"));
        let done = json!({"prompt": "sleep", "receiverThreadIds": ["t1", " "]});
        found.update(Some(&done));
        found.update(Some(&done));
        assert_eq!(found.threads, ["t1"]);
        // Waiting on a subagent starts none.
        assert!(Subagent::of_call("wait", Some(&done)).is_none());
    }
}
