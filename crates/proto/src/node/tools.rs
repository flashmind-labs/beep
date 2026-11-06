//! `tools/1`: the Beep MCP server's tools, which coding agents on a node
//! call to work across the user's machines. Unlike the other protocols the
//! node opens these streams, on its connection to the server, and only
//! when the welcome said the server takes them (`Down::Welcome::tools`).
//! Each stream starts with `Open`, then carries one `Request` and its
//! `Reply`. The server runs the call on the machine it names, so the
//! user's rules for their machines apply as for Beep's own tools.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

pub const PROTOCOL: &str = "tools/1";

/// How the text of a call the user's rules refuse starts, as the server and a node that stands in
/// for it both say it.
pub const REFUSED: &str = "Not done: the user set";

/// The tools the server runs. `ask_user` is not among them: the node puts
/// that question to the user itself.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    IntoStaticStr,
    EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Tool {
    ListMachines,
    RunOn,
    SyncProject,
    SendFiles,
    JobStatus,
    Cancel,
    MachineLog,
    /// No longer sent: coding agents get no memories, which are the
    /// assistant's. Kept so the variants after it keep their place.
    Remember,
    Recall,
    UpdateMemory,
    Forget,
    /// Starts a coding agent linked to the calling session, on the machine
    /// that answers. Between nodes only: the server never runs it.
    StartAgent,
    AgentStatus,
    SendToAgent,
    StopAgent,
    ListAgents,
    /// A node asks the node of a session that started its agent whether it still
    /// keeps that agent, so one whose parent is gone closes. Between nodes only.
    AgentParent,
    /// A node asks another how many live agents run there under a root session,
    /// to hold the limit across machines. Between nodes only.
    AgentCount,
    /// No longer sent: a node read the user's standing rules with it for a
    /// session's first prompt. Kept so the variants after it keep their place.
    StandingRules,
    /// Not a tool agents see: a node asks another how its calls to each code
    /// host went, to pick the machine that watches a project's CI runs.
    /// `Done` with a `ForgeReach` as JSON. Between nodes only.
    ForgeReach,
    /// A tool a newer node calls that this server does not know.
    #[serde(other)]
    Unknown,
}

/// What the user said to a call that asked first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approved {
    /// Not asked yet: the server checks the user's rules.
    #[default]
    No,
    /// The user allowed this one call.
    Once,
    /// The user allowed it from now on: the server keeps that in the rules.
    Always,
}

/// What the node sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// The node's id of the agent session that called the tool.
    pub session: String,
    pub tool: Tool,
    /// The call's arguments as the agent sent them, as JSON text.
    pub args: String,
    #[serde(default)]
    pub approved: Approved,
}

/// One argument an approval shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub label: String,
    pub value: String,
}

/// What the server answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// The call ran: `text` is its result for the agent, `failed` when it
    /// did not work.
    Done { text: String, failed: bool },
    /// The user's rules want them asked first. Send the request again with
    /// `approved` once they allow it.
    Ask {
        /// The app the call acts in, such as "Your machines".
        app: String,
        /// What it does, such as "run on".
        action: String,
        fields: Vec<Field>,
    },
}

/// What `ForgeReach` answers: how this node's calls to each code host went,
/// by host name such as `github.com`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeReach {
    #[serde(default)]
    pub hosts: std::collections::BTreeMap<String, HostReach>,
}

/// How one machine's calls to one code host went since it started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostReach {
    /// A call succeeded.
    #[serde(default)]
    pub reached: bool,
    /// Calls that failed in a row, the latest included.
    #[serde(default)]
    pub failures: u32,
}

impl HostReach {
    /// Failures in a row after which a machine counts as not reaching the host.
    pub const GIVE_UP: u32 = 3;

    /// Which machines to ask the host from first: 0 reaches it, 1 was not
    /// tried yet (or failed fewer than `GIVE_UP` times since starting), 2
    /// keeps failing.
    pub fn rank(self) -> u8 {
        if self.failures >= Self::GIVE_UP {
            2
        } else if self.reached {
            0
        } else {
            1
        }
    }

    /// Notes one call's outcome.
    pub fn record(&mut self, ok: bool) {
        if ok {
            self.reached = true;
            self.failures = 0;
        } else {
            self.failures = self.failures.saturating_add(1);
        }
    }
}
