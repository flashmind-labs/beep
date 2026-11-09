//! What a step of a tool that takes an `action` did, read back from its
//! result. A saved step keeps the tool's name and output but not its
//! arguments, so the name alone says "memory" where the run said "Recalling
//! memories".

use serde_json::Value;

const MEMORY: &str = "memory";
const BACKGROUND: &str = "background";

/// A key the result of `tool` has for one action, and the label the server
/// gives that action while it runs.
struct Marked {
    tool: &'static str,
    key: &'static str,
    label: &'static str,
}

const fn marked(tool: &'static str, key: &'static str, label: &'static str) -> Marked {
    Marked { tool, key, label }
}

const MARKED: &[Marked] = &[
    marked(MEMORY, "stored", "Remembering"),
    marked(MEMORY, "updated", "Updating a memory"),
    marked(MEMORY, "forgotten", "Forgetting a memory"),
    marked("skill", "created", "Saving a skill"),
    marked("skill", "updated", "Updating a skill"),
    marked("skill", "body", "Loading a skill"),
    marked("habit", "created", "Creating a habit"),
    marked("habit", "logged", "Logging a habit"),
    marked("habit", "habits", "Checking habits"),
    marked("habit", "archived", "Deleting a habit"),
];

/// The label of the action a `name` step with result `output` ran, in the
/// present tense as the server words it, or `None` when its result does not
/// say and the tool's name is all there is.
pub fn action_label(name: &str, output: &str) -> Option<&'static str> {
    if name == BACKGROUND {
        return if output.starts_with("Started agent ") {
            Some("Starting an agent")
        } else if output.starts_with("Job ") && output.contains(" is running") {
            Some("Running a job")
        } else {
            None
        };
    }

    let value: Value = serde_json::from_str(output).ok()?;

    if let Value::Array(items) = &value {
        if name != MEMORY {
            return None;
        }
        // A memory's history lists its old versions with when each was replaced.
        let history = items.iter().any(|item| item.get("replaced").is_some());
        return Some(if history {
            "Checking a memory's history"
        } else {
            "Recalling memories"
        });
    }

    MARKED
        .iter()
        .find(|marked| marked.tool == name && value.get(marked.key).is_some())
        .map(|marked| marked.label)
}

#[cfg(test)]
#[path = "../tests/chat_tool_labels.rs"]
mod tests;
