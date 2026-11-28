//! The names of the session protocols the node serves itself. `crate::protocol` holds the ones a host adds.

// Protocol names are only routed on machines hosting sessions.
#[cfg(feature = "host")]
use strum::{AsRefStr, Display, EnumString};

#[cfg(feature = "host")]
#[derive(Debug, Clone, PartialEq, Eq, EnumString, AsRefStr, Display)]
pub(crate) enum SessionProtocol {
    #[strum(serialize = "metrics/1")]
    Metrics,
    #[strum(serialize = "metrics/2")]
    MetricsQuery,
    #[strum(serialize = "log-query/1")]
    LogQuery,
    #[strum(serialize = "report/1")]
    Report,
    #[strum(serialize = "status/1")]
    Status,
    #[strum(serialize = "version/1")]
    Version,
    #[strum(serialize = "tools/1")]
    Tools,
    #[strum(serialize = "shell/1")]
    Shell,
    #[strum(serialize = "skills/1")]
    Skills,
    #[strum(serialize = "skill-library/1")]
    SkillLibrary,
    #[strum(serialize = "process/1")]
    Tasks,
    #[strum(serialize = "terminal/1")]
    Terminal,
    #[strum(serialize = "device/1")]
    Device,
    #[strum(serialize = "device-video/1")]
    DeviceVideo,
    #[strum(serialize = "sign-in/1")]
    SignIn,
    #[strum(serialize = "mcp-servers/1")]
    McpServers,
    #[strum(serialize = "tool-updates/1")]
    ToolCheck,
    #[strum(serialize = "tool-update/1")]
    ToolUpdate,
    #[strum(serialize = "tool-update/flsh")]
    FlshUpdate,
    #[strum(serialize = "setup/1")]
    Setup,
    #[strum(serialize = "events/1")]
    Events,
    #[strum(serialize = "peer-commands/1")]
    Commands,
    #[strum(serialize = "peer-subscriptions/1")]
    Subscriptions,
    #[strum(serialize = "agent-view/1")]
    AgentFollow,
    #[strum(serialize = "agent-transcripts/1")]
    Transcripts,
    #[strum(serialize = "agents/1")]
    Agents,
    #[strum(default, transparent)]
    Other(String),
}
