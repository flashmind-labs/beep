//! A call to the `beep` server as a transcript shows it (`Tool`). Agents
//! title it by the tool's name alone, so the node words it from its arguments,
//! such as `Beep: Run on build-box: cargo test`; the apps read it back.

use super::{SERVER_NAME, machines::command_name};
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
/// - `List devices`, `Boot <device> on <machine>`, `Shut down …`, `Show …`,
///   `Capture …`, `Tap … on <machine>: <x>, <y>`, `Swipe …: <x>, <y> to
///   <x2>, <y2>`, `Type into …: <text>`, `Press <key> on <device> on
///   <machine>`
/// - `Read status`, `Read logs on <machine>`, `Read metrics`, `File a
///   report: <description>`
/// - `List Git hosts`, `Create project: <name>`, `Clone project: <url>`,
///   `Find projects`, `Import projects`
///
/// A grouped tool of older sessions (`machines`, `run`, `files`, `agent`,
/// `beep`, `device`) takes the words of the tool its action calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Tool {
    ListMachines,
    ListProjects,
    HarnessInfo,
    #[strum(serialize = "read_session_transcript")]
    ReadTranscript,
    #[strum(serialize = "remote_run")]
    RunOn,
    SyncProject,
    SendFiles,
    #[strum(to_string = "remote_run_status", serialize = "job_status")]
    JobStatus,
    #[strum(to_string = "remote_run_cancel", serialize = "cancel")]
    Cancel,
    StartAgent,
    AgentStatus,
    SendToAgent,
    StopAgent,
    ListAgents,
    AskUser,
    Review,
    SubmitReview,
    DeviceList,
    DeviceBoot,
    DeviceShutdown,
    DeviceShow,
    DeviceScreenshot,
    DeviceTap,
    DeviceSwipe,
    DeviceType,
    DevicePress,
    DeviceLogs,
    DeviceUi,
    DeviceInstall,
    DeviceLaunch,
    DeviceReset,
    BeepStatus,
    BeepLogs,
    BeepMetrics,
    #[strum(to_string = "beep_report", serialize = "file_report")]
    FileReport,
    ProjectsHosts,
    ProjectsCreate,
    ProjectsClone,
    ProjectsFound,
    ProjectsImport,
    Machines,
    Run,
    Files,
    Agent,
    Beep,
    Device,
    Plugins,
}

/// The tools the grouped tools of older sessions called, by their action.
const GROUPS: [(Tool, &[(&str, Tool)]); 5] = [
    (
        Tool::Machines,
        &[
            ("list", Tool::ListMachines),
            ("projects", Tool::ListProjects),
            ("harness", Tool::HarnessInfo),
        ],
    ),
    (
        Tool::Run,
        &[
            ("start", Tool::RunOn),
            ("status", Tool::JobStatus),
            ("cancel", Tool::Cancel),
        ],
    ),
    (
        Tool::Files,
        &[("sync", Tool::SyncProject), ("send", Tool::SendFiles)],
    ),
    (
        Tool::Agent,
        &[
            ("start", Tool::StartAgent),
            ("status", Tool::AgentStatus),
            ("send", Tool::SendToAgent),
            ("stop", Tool::StopAgent),
            ("list", Tool::ListAgents),
            ("transcript", Tool::ReadTranscript),
        ],
    ),
    (
        Tool::Beep,
        &[
            ("status", Tool::BeepStatus),
            ("logs", Tool::BeepLogs),
            ("metrics", Tool::BeepMetrics),
            ("report", Tool::FileReport),
        ],
    ),
];

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

    /// The tool a grouped tool's `action` calls, from its arguments as far
    /// as they have come; itself when it is not grouped or names no action.
    pub(crate) fn resolved(self, input: Option<&Value>) -> Self {
        let Some(action) = input
            .map(arguments)
            .and_then(|args| args.get("action")?.as_str())
            .map(str::trim)
        else {
            return self;
        };

        if self == Self::Device {
            return format!("device_{action}").parse().unwrap_or(self);
        }

        GROUPS
            .iter()
            .find(|(group, _)| *group == self)
            .and_then(|(_, actions)| actions.iter().find(|(name, _)| *name == action))
            .map_or(self, |(_, tool)| *tool)
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
            Self::ListProjects => with("List projects".to_owned(), text("project")),
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
            Self::ListAgents => match text("project") {
                Some(project) => format!("{PREFIX}List agents in {project}"),
                None => format!("{PREFIX}List agents"),
            },
            Self::AskUser => with("Ask".to_owned(), text("question")),
            Self::Review => match (text("id"), text("base")) {
                (Some(id), _) => format!("{PREFIX}Check review {id}"),
                (None, Some(base)) => format!("{PREFIX}Review changes against {base}"),
                (None, None) => format!("{PREFIX}Review changes"),
            },
            Self::SubmitReview => format!("{PREFIX}Submit review"),
            Self::BeepStatus => format!("{PREFIX}Read status"),
            Self::BeepLogs => match machine {
                Some(machine) => format!("{PREFIX}Read logs on {machine}"),
                None => format!("{PREFIX}Read logs"),
            },
            Self::BeepMetrics => match machine {
                Some(machine) => format!("{PREFIX}Read metrics on {machine}"),
                None => format!("{PREFIX}Read metrics"),
            },
            Self::FileReport => with("File a report".to_owned(), text("description")),
            Self::ProjectsHosts => format!("{PREFIX}List Git hosts"),
            Self::ProjectsCreate => with("Create project".to_owned(), text("name")),
            Self::ProjectsClone => with("Clone project".to_owned(), text("url")),
            Self::ProjectsFound => format!("{PREFIX}Find projects"),
            Self::ProjectsImport => format!("{PREFIX}Import projects"),
            Self::Machines
            | Self::Run
            | Self::Files
            | Self::Agent
            | Self::Beep
            | Self::Device => match self.resolved(input) {
                tool if tool != self => return tool.title(input),
                _ => format!("{PREFIX}Use {}", Into::<&'static str>::into(self)),
            },
            Self::Plugins => format!("{PREFIX}Use plugins"),
            Self::DeviceList => format!("{PREFIX}List devices"),
            device @ (Self::DeviceBoot
            | Self::DeviceShutdown
            | Self::DeviceShow
            | Self::DeviceScreenshot
            | Self::DeviceTap
            | Self::DeviceSwipe
            | Self::DeviceType
            | Self::DevicePress
            | Self::DeviceLogs
            | Self::DeviceUi
            | Self::DeviceInstall
            | Self::DeviceLaunch
            | Self::DeviceReset) => {
                let number = |key: &str| {
                    args.and_then(|args| args.get(key)?.as_number())
                        .map_or_else(|| "?".to_owned(), ToString::to_string)
                };
                let name = text("device").unwrap_or("a device");
                let machine = machine.map_or_else(|| "a machine".to_owned(), command_name);
                let at = format!("{name} on {machine}");

                match device {
                    Self::DeviceBoot => format!("{PREFIX}Boot {at}"),
                    Self::DeviceShutdown => format!("{PREFIX}Shut down {at}"),
                    Self::DeviceShow => format!("{PREFIX}Show {at}"),
                    Self::DeviceScreenshot => format!("{PREFIX}Capture {at}"),
                    Self::DeviceTap => {
                        format!("{PREFIX}Tap {at}: {}, {}", number("x"), number("y"))
                    }
                    Self::DeviceSwipe => format!(
                        "{PREFIX}Swipe {at}: {}, {} to {}, {}",
                        number("x"),
                        number("y"),
                        number("x2"),
                        number("y2")
                    ),
                    Self::DeviceType => with(format!("Type into {at}"), text("text")),
                    Self::DevicePress => {
                        format!("{PREFIX}Press {} on {at}", text("key").unwrap_or("a key"))
                    }
                    _ => format!("{PREFIX}Use {at}"),
                }
            }
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
#[path = "../../tests/agents_mcp_calls.rs"]
mod tests;
