//! A call to the `beep` server as a session's transcript shows it. Agents
//! title these calls by the tool's name alone (`mcp__beep__run_on`), so the
//! node words them from their arguments instead: `Beep: Run on build-box:
//! cargo test`. The apps read the same words back into a row of their own.
//!
//! The forms, after the `Beep: ` prefix:
//!
//! - `List machines`
//! - `Run on <machine>: <command>`
//! - `Start <agent> on <machine>: <prompt>` (`an agent` when none is named)
//! - `Sync <project>`, or `Sync <project> to <machine>`
//! - `Send <n> files to <machine>`
//! - `Check job <id>`, `Cancel job <id>`
//! - `Ask: <question>`

use super::{SERVER_NAME, catalog::ASK_USER};
use serde_json::{Map, Value};

/// What every title of a `beep` call starts with.
pub(crate) const PREFIX: &str = "Beep: ";

/// One of the `beep` server's tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tool {
    ListMachines,
    RunOn,
    SyncProject,
    SendFiles,
    JobStatus,
    Cancel,
    AskUser,
}

impl Tool {
    /// The tool by its name in `tools/list`.
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "list_machines" => Self::ListMachines,
            "run_on" => Self::RunOn,
            "sync_project" => Self::SyncProject,
            "send_files" => Self::SendFiles,
            "job_status" => Self::JobStatus,
            "cancel" => Self::Cancel,
            ASK_USER => Self::AskUser,
            _ => return None,
        })
    }

    /// The tool a Claude Code tool name calls: `mcp__beep__run_on`.
    pub(crate) fn of_name(name: &str) -> Option<Self> {
        let rest = name.strip_prefix("mcp__")?;
        let tool = rest.strip_prefix(SERVER_NAME)?.strip_prefix("__")?;
        Self::named(tool)
    }

    /// The tool an ACP tool call runs, from Claude Code's `_meta`, its
    /// title (`mcp__beep__run_on`, Codex's `beep/run_on`, Gemini's
    /// `run_on (beep MCP Server)`) or an input that names the server, as
    /// Codex's does.
    pub(crate) fn of_call(
        title: &str,
        meta: Option<&Map<String, Value>>,
        input: Option<&Value>,
    ) -> Option<Self> {
        let named = meta
            .and_then(|meta| meta.get("claudeCode")?.get("toolName")?.as_str())
            .and_then(Self::of_name);
        named
            .or_else(|| Self::of_title(title))
            .or_else(|| Self::of_input(input?))
    }

    fn of_title(title: &str) -> Option<Self> {
        let title = title.trim();
        if let Some(tool) = Self::of_name(title) {
            return Some(tool);
        }
        let bare = title.strip_prefix("Tool: ").unwrap_or(title);
        if let Some(rest) = bare.strip_prefix(SERVER_NAME)
            && let Some(tool) = rest.strip_prefix('/').or_else(|| rest.strip_prefix('.'))
        {
            return Self::named(tool);
        }
        let (tool, server) = title.split_once(" (")?;
        let server = server.strip_suffix(" MCP Server)")?;
        (server == SERVER_NAME).then(|| Self::named(tool)).flatten()
    }

    fn of_input(input: &Value) -> Option<Self> {
        if input.get("server")?.as_str()? != SERVER_NAME {
            return None;
        }
        Self::named(input.get("tool")?.as_str()?)
    }

    /// The call's title, from its arguments as far as they have come.
    pub(crate) fn title(self, input: Option<&Value>) -> String {
        let args = input.map(arguments);
        let text = |key: &str| {
            args.and_then(|args| args.get(key)?.as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty())
        };
        let machine = text("machine");
        let on = |verb: &str| match machine {
            Some(machine) => format!("{verb} on {machine}"),
            None => format!("{verb} on a machine"),
        };
        let with = |head: String, detail: Option<&str>| match detail {
            Some(detail) => format!("{PREFIX}{head}: {detail}"),
            None => format!("{PREFIX}{head}"),
        };
        match self {
            Self::ListMachines => format!("{PREFIX}List machines"),
            Self::RunOn => match (text("command"), text("prompt")) {
                (None, Some(prompt)) => {
                    let agent = text("agent").unwrap_or("an agent");
                    with(on(&format!("Start {agent}")), Some(prompt))
                }
                (command, _) => with(on("Run"), command),
            },
            Self::SyncProject => {
                let project = text("project").unwrap_or("a project");
                match machine {
                    Some(machine) => format!("{PREFIX}Sync {project} to {machine}"),
                    None => format!("{PREFIX}Sync {project}"),
                }
            }
            Self::SendFiles => {
                let count = args
                    .and_then(|args| args.get("paths")?.as_array())
                    .map_or(0, Vec::len);
                let files = if count == 1 { "file" } else { "files" };
                let to = machine.unwrap_or("a machine");
                format!("{PREFIX}Send {count} {files} to {to}")
            }
            Self::JobStatus => format!("{PREFIX}Check job {}", text("id").unwrap_or("")),
            Self::Cancel => format!("{PREFIX}Cancel job {}", text("id").unwrap_or("")),
            Self::AskUser => with("Ask".to_owned(), text("question")),
        }
        .trim_end()
        .to_owned()
    }
}

/// The tool's own arguments: Codex wraps them with the server and tool.
fn arguments(input: &Value) -> &Value {
    match input.get("arguments") {
        Some(args) if args.is_object() && input.get("tool").is_some() => args,
        _ => input,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn calls_are_found_however_the_agent_names_them() {
        assert_eq!(Tool::of_name("mcp__beep__run_on"), Some(Tool::RunOn));
        assert_eq!(Tool::of_name("mcp__beeper__run_on"), None);
        assert_eq!(Tool::of_name("mcp__other__run_on"), None);
        let meta = json!({"claudeCode": {"toolName": "mcp__beep__send_files"}});
        assert_eq!(
            Tool::of_call("Other", meta.as_object(), None),
            Some(Tool::SendFiles)
        );
        assert_eq!(
            Tool::of_call("mcp__beep__list_machines", None, None),
            Some(Tool::ListMachines)
        );
        assert_eq!(
            Tool::of_call("Tool: beep/job_status", None, None),
            Some(Tool::JobStatus)
        );
        assert_eq!(
            Tool::of_call("cancel (beep MCP Server)", None, None),
            Some(Tool::Cancel)
        );
        let codex = json!({"server": "beep", "tool": "ask_user", "arguments": {"question": "Go?"}});
        assert_eq!(
            Tool::of_call("Working", None, Some(&codex)),
            Some(Tool::AskUser)
        );
        assert_eq!(Tool::of_call("Run cargo test", None, None), None);
        assert_eq!(Tool::of_call("beep/unknown", None, None), None);
    }
}
