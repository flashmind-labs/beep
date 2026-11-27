//! A call to the `beep` server as a transcript shows it (`Tool`). Agents
//! title it by the tool's name alone, so the node words it from its arguments,
//! such as `Beep: Run on build-box: cargo test`; the apps read it back.

use serde_json::{Map, Value};

/// The server's name, as agents prefix its tools: `mcp__beep__remote_run`.
pub const SERVER_NAME: &str = "beep";

/// A machine as the agent names it: lower case, one word, with apostrophes
/// dropped and other symbols and spaces as single dashes.
pub fn command_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| !matches!(c, '\'' | '\u{2019}'))
        .flat_map(char::to_lowercase)
        .collect();

    kept.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// What every title of a `beep` call starts with.
pub(crate) const PREFIX: &str = "Beep: ";

/// One of the `beep` server's tools.
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
    StartThread,
    ThreadStatus,
    SendToThread,
    StopThread,
    ListThreads,
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
    TeamJoin,
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

    /// The tool an ACP tool call runs, from its `name`, Claude Code's `_meta`,
    /// its title (`mcp__beep__remote_run`, flsh's `mcp__beep__start_agent: ...`,
    /// Codex's `beep/remote_run`) or an input that names the server, as Codex's does.
    pub(crate) fn of_call(
        name: Option<&str>,
        title: &str,
        meta: Option<&Map<String, Value>>,
        input: Option<&Value>,
    ) -> Option<Self> {
        let named = name.and_then(Self::of_name).or_else(|| {
            meta.and_then(|meta| meta.get("claudeCode")?.get("toolName")?.as_str())
                .and_then(Self::of_name)
        });

        named
            .or_else(|| Self::of_title(title))
            .or_else(|| Self::try_from(input?).ok())
    }

    fn of_title(title: &str) -> Option<Self> {
        let title = title.trim();
        let head = title.split_once(':').map_or(title, |(head, _)| head.trim());

        if let Some(tool) = Self::of_name(head) {
            return Some(tool);
        }

        let bare = title.strip_prefix("Tool: ").unwrap_or(title);
        let bare = bare.split_once(':').map_or(bare, |(head, _)| head.trim());

        let rest = bare.strip_prefix(SERVER_NAME)?;
        let tool = rest.strip_prefix('/').or_else(|| rest.strip_prefix('.'))?;

        tool.parse().ok()
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
        let id = text("id").unwrap_or("");
        let on = |verb: &str| format!("{verb} on {}", machine.unwrap_or("a machine"));
        let on_opt = |what: &str| match machine {
            Some(machine) => format!("{what} on {machine}"),
            None => what.to_owned(),
        };
        let with = |head: &str, detail: Option<&str>| match detail {
            Some(detail) => format!("{head}: {detail}"),
            None => head.to_owned(),
        };

        let body = match self {
            Self::ListProjects => with("List projects", text("project")),
            Self::ListMachines => "List machines".to_owned(),
            Self::HarnessInfo => {
                on_opt(&format!("Query {}", text("harness").unwrap_or("a harness")))
            }
            Self::ReadTranscript => with(&on("Read transcript"), text("session")),
            Self::RunOn => with(&on("Run"), text("command")),
            Self::SyncProject => {
                let project = text("project").unwrap_or("a project");

                match machine {
                    Some(machine) => format!("Sync {project} to {machine}"),
                    None => format!("Sync {project}"),
                }
            }
            Self::SendFiles => {
                let count = args
                    .and_then(|args| args.get("paths")?.as_array())
                    .map_or(0, Vec::len);
                let files = if count == 1 { "file" } else { "files" };

                format!("Send {count} {files} to {}", machine.unwrap_or("a machine"))
            }
            Self::JobStatus => format!("Check job {id}"),
            Self::Cancel => format!("Cancel job {id}"),
            Self::StartAgent => with(
                &on_opt(&format!("Start {} agent", text("harness").unwrap_or("an"))),
                text("prompt"),
            ),
            Self::AgentStatus => format!("Check agent {id}"),
            Self::SendToAgent => with(&format!("Message agent {id}"), text("text")),
            Self::StopAgent => format!("Stop agent {id}"),
            Self::ListAgents => match text("project") {
                Some(project) => format!("List agents in {project}"),
                None => "List agents".to_owned(),
            },
            Self::StartThread => with("Start thread", text("prompt")),
            Self::ThreadStatus => format!("Check thread {id}"),
            Self::SendToThread => with(&format!("Message thread {id}"), text("text")),
            Self::StopThread => format!("Stop thread {id}"),
            Self::ListThreads => "List threads".to_owned(),
            Self::AskUser => with("Ask", text("question")),
            Self::Review => match (text("id"), text("base")) {
                (Some(id), _) => format!("Check review {id}"),
                (None, Some(base)) => format!("Review changes against {base}"),
                (None, None) => "Review changes".to_owned(),
            },
            Self::SubmitReview => "Submit review".to_owned(),
            Self::BeepStatus => "Read status".to_owned(),
            Self::BeepLogs => on_opt("Read logs"),
            Self::BeepMetrics => on_opt("Read metrics"),
            Self::FileReport => with("File a report", text("description")),
            Self::TeamJoin => "Join a team".to_owned(),
            Self::ProjectsHosts => "List Git hosts".to_owned(),
            Self::ProjectsCreate => with("Create project", text("name")),
            Self::ProjectsClone => with("Clone project", text("url")),
            Self::ProjectsFound => "Find projects".to_owned(),
            Self::ProjectsImport => "Import projects".to_owned(),
            Self::Machines | Self::Run | Self::Files | Self::Agent | Self::Beep | Self::Device => {
                match self.resolved(input) {
                    tool if tool != self => return tool.title(input),
                    _ => format!("Use {}", Into::<&'static str>::into(self)),
                }
            }
            Self::Plugins => "Use plugins".to_owned(),
            Self::DeviceList => "List devices".to_owned(),
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
                let num = |key: &str| {
                    args.and_then(|args| args.get(key)?.as_number())
                        .map_or_else(|| "?".to_owned(), ToString::to_string)
                };
                let machine = machine.map_or_else(|| "a machine".to_owned(), command_name);
                let at = format!("{} on {machine}", text("device").unwrap_or("a device"));

                match device {
                    Self::DeviceBoot => format!("Boot {at}"),
                    Self::DeviceShutdown => format!("Shut down {at}"),
                    Self::DeviceShow => format!("Show {at}"),
                    Self::DeviceScreenshot => format!("Capture {at}"),
                    Self::DeviceTap => format!("Tap {at}: {}, {}", num("x"), num("y")),
                    Self::DeviceSwipe => format!(
                        "Swipe {at}: {}, {} to {}, {}",
                        num("x"),
                        num("y"),
                        num("x2"),
                        num("y2")
                    ),
                    Self::DeviceType => with(&format!("Type into {at}"), text("text")),
                    Self::DevicePress => {
                        format!("Press {} on {at}", text("key").unwrap_or("a key"))
                    }
                    _ => format!("Use {at}"),
                }
            }
        };

        format!("{PREFIX}{}", body.trim_end())
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
#[path = "tests/calls.rs"]
mod tests;
