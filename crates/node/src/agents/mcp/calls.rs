//! A call to the `beep` server as a transcript shows it (`Tool`). Agents
//! title it by the tool's name alone, so the node words it from its arguments,
//! such as `Beep: Run on build-box: cargo test`; the apps read it back.

use super::SERVER_NAME;
use serde_json::{Map, Value};

/// What every title of a `beep` call starts with.
pub(crate) const PREFIX: &str = "Beep: ";

/// One of the `beep` server's tools. The words its calls take, after
/// `PREFIX`:
///
/// - `List machines`
/// - `Run on <machine>: <command>`
/// - `Start <agent> on <machine>: <prompt>` (`an agent` when none is named)
/// - `Start <harness> agent on <machine>: <prompt>`, `Check agent <id>`,
///   `Message agent <id>: <text>`, `Stop agent <id>`, `List agents`
/// - `Sync <project>`, or `Sync <project> to <machine>`
/// - `Send <n> files to <machine>`
/// - `Check job <id>`, `Cancel job <id>`
/// - `Ask: <question>`
/// - `Review changes`, `Review changes against <base>`, `Check review <id>`
/// - `Submit review`
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Tool {
    ListMachines,
    HarnessInfo,
    #[strum(serialize = "read_session_transcript")]
    ReadTranscript,
    #[strum(serialize = "remote_run")]
    RunOn,
    SyncProject,
    SendFiles,
    JobStatus,
    Cancel,
    StartAgent,
    AgentStatus,
    SendToAgent,
    StopAgent,
    ListAgents,
    AskUser,
    Review,
    SubmitReview,
}

impl Tool {
    /// The tool a Claude Code tool name calls: `mcp__beep__remote_run`.
    pub(crate) fn of_name(name: &str) -> Option<Self> {
        let rest = name.strip_prefix("mcp__")?;
        let tool = rest.strip_prefix(SERVER_NAME)?.strip_prefix("__")?;
        tool.parse().ok()
    }

    /// The tool an ACP tool call runs, from Claude Code's `_meta`, its
    /// title (`mcp__beep__remote_run`, Codex's `beep/remote_run`, Gemini's
    /// `remote_run (beep MCP Server)`) or an input that names the server, as
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
            .or_else(|| Self::try_from(input?).ok())
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
            return tool.parse().ok();
        }
        let (tool, server) = title.split_once(" (")?;
        let server = server.strip_suffix(" MCP Server)")?;
        (server == SERVER_NAME).then(|| tool.parse().ok()).flatten()
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
            Self::HarnessInfo => {
                let harness = text("harness").unwrap_or("a harness");
                match machine {
                    Some(machine) => format!("{PREFIX}Query {harness} on {machine}"),
                    None => format!("{PREFIX}Query {harness}"),
                }
            }
            Self::ReadTranscript => with(on("Read transcript"), text("session")),
            Self::RunOn => with(on("Run"), text("command")),
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
            Self::StartAgent => {
                let harness = text("harness").unwrap_or("an");
                let on = match machine {
                    Some(machine) => format!("{harness} agent on {machine}"),
                    None => format!("{harness} agent"),
                };
                with(format!("Start {on}"), text("prompt"))
            }
            Self::AgentStatus => format!("{PREFIX}Check agent {}", text("id").unwrap_or("")),
            Self::SendToAgent => with(
                format!("Message agent {}", text("id").unwrap_or("")),
                text("text"),
            ),
            Self::StopAgent => format!("{PREFIX}Stop agent {}", text("id").unwrap_or("")),
            Self::ListAgents => format!("{PREFIX}List agents"),
            Self::AskUser => with("Ask".to_owned(), text("question")),
            Self::Review => match (text("id"), text("base")) {
                (Some(id), _) => format!("{PREFIX}Check review {id}"),
                (None, Some(base)) => format!("{PREFIX}Review changes against {base}"),
                (None, None) => format!("{PREFIX}Review changes"),
            },
            Self::SubmitReview => format!("{PREFIX}Submit review"),
        }
        .trim_end()
        .to_owned()
    }
}

impl TryFrom<&Value> for Tool {
    type Error = strum::ParseError;

    fn try_from(input: &Value) -> Result<Self, Self::Error> {
        if input.get("server").and_then(Value::as_str) != Some(SERVER_NAME) {
            return Err(strum::ParseError::VariantNotFound);
        }
        input
            .get("tool")
            .and_then(Value::as_str)
            .ok_or(strum::ParseError::VariantNotFound)?
            .parse()
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
        assert_eq!(Tool::of_name("mcp__beep__remote_run"), Some(Tool::RunOn));
        assert_eq!(Tool::of_name("mcp__beeper__remote_run"), None);
        assert_eq!(Tool::of_name("mcp__other__remote_run"), None);
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
        assert_eq!(Tool::of_name("mcp__beep__remember"), None);
    }
}
