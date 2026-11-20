//! `tools/1`: the Beep MCP tools, which coding agents on a node call to
//! work across the user's machines. The node runs each `Request` itself,
//! reaching the machine it names over the mesh, and answers with a `Reply`.

use crate::{
    SessionId,
    team::{Card, ForgeLogin, TeamRecord, TeamRepo},
};
use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

pub const PROTOCOL: &str = "tools/1";

/// The tools a node runs. `ask_user` is not among them: the node puts
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
    /// No longer sent: coding agents cannot read machine logs. Kept so the
    /// variants after it keep their place.
    MachineLog,
    /// No longer sent: coding agents get no memories, which are the
    /// assistant's. Kept so the variants after it keep their place.
    Remember,
    Recall,
    UpdateMemory,
    Forget,
    /// Starts a coding agent linked to the calling session, on the machine
    /// that answers.
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
    /// Starts a temporary reviewer of the calling session's changes and
    /// gives its findings, or polls one. On the caller's own node only.
    Review,
    /// A reviewer hands in its review. On the reviewer's own node only.
    SubmitReview,
    /// A child's node tells the node of the session that started it that
    /// the child finished a turn or waits on the user, for a note in the
    /// parent's session. Between nodes only.
    AgentNote,
    /// Beep itself, for an agent debugging it: every machine's status, a
    /// machine's log, and a problem report sent. On the caller's own node only.
    BeepStatus,
    BeepLogs,
    FileReport,
    /// The models and capabilities of a coding harness on this machine.
    HarnessInfo,
    /// Retained metrics for a bounded period, on the caller's own node.
    BeepMetrics,
    /// Projects and their checkouts from this node's replicated mesh state.
    ListProjects,
    /// Beep plugins, which run in a desktop: its `args` hold the `plugins`
    /// tool's arguments. A node forwards it to a desktop of the mesh.
    Plugins,
    /// Projects the home assistant creates, clones and imports, which a
    /// desktop does: its `args` hold the `projects` tool's arguments. A node
    /// forwards it to a desktop of the mesh.
    Projects,
    /// A desktop asks the machine with a project's checkout to join that
    /// project's team: `TeamJoinArgs`, then `Done` with the `TeamRecord` as JSON.
    TeamJoin,
    /// A node hands a team record it changed to a desktop, which writes it:
    /// `TeamRosterArgs`. Nodes cannot write the mesh's state.
    TeamRoster,
    /// A node hands a contact's invite to a team to a desktop, to show with
    /// Accept: `TeamInviteArgs`.
    TeamInvite,
    /// The team of the calling session's project: its members, their
    /// cards and the open issues with their assignees. A node forwards it to
    /// its mesh's gateway for the team: `TeamArgs`.
    Team,
    /// Sends a teammate's agent or a teammate an ask, or answers one. A
    /// node forwards it to its mesh's gateway for the team: `TeamAskArgs`.
    TeamAsk,
    /// A gateway hands an ask or its answer to the session it is for, on
    /// the machine that runs it: `TeamDeliverArgs`. Between nodes only.
    TeamDeliver,
    /// A gateway starts an agent on an issue assigned to the user, on a
    /// machine of the mesh with the project: `PickupStart`, then `Done`
    /// with the session's card id. Between nodes only.
    TeamPickup,
    /// What this mesh's apps ask of its teams: `TeamAppArgs`. A node sends
    /// what only a team's gateway knows or may send on to that gateway.
    TeamApp,
    /// A tool a newer node calls that this one does not know.
    #[serde(other)]
    Unknown,
}

/// What the user said to a call that asked first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approved {
    /// Not asked yet.
    #[default]
    No,
    /// The user allowed this one call.
    Once,
    /// The user allowed it from now on.
    Always,
}

/// What the node sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// The node's id of the agent session that called the tool.
    pub session: SessionId,
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

/// What the node answers.
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

/// `Tool::TeamJoin`'s arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamJoinArgs {
    /// The `beepteam1` ticket.
    pub ticket: String,
}

/// `Tool::TeamRoster`'s arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamRosterArgs {
    pub record: TeamRecord,
}

/// `Tool::TeamInvite`'s arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamInviteArgs {
    /// The contact's mesh.
    pub from: EndpointId,
    pub repo: TeamRepo,
    /// The `beepteam1` ticket accepting runs.
    pub ticket: String,
}

/// `Tool::TeamApp`'s arguments, by `action`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TeamAppArgs {
    /// `repo`'s Needs you cards and teammates' cards: `Done` with
    /// `TeamCards` as JSON.
    Cards { repo: TeamRepo },
    /// Drops the Needs you card `card` of `repo`.
    Dismiss { repo: TeamRepo, card: String },
    /// Tells every member of another mesh that `login` is off `repo`'s team.
    Remove { repo: TeamRepo, login: ForgeLogin },
    /// Tells every member of another mesh that this mesh leaves `repo`'s team.
    Leave { repo: TeamRepo },
    /// Sends a contact, at `devices`, an invite to `repo`. It goes from the
    /// gateway of `via`, a team shared with them, whose devices they know.
    Invite {
        via: TeamRepo,
        repo: TeamRepo,
        ticket: String,
        devices: Vec<EndpointAddr>,
    },
    /// The contacts' invites this device keeps: `Done` with the
    /// `TeamInviteArgs` as JSON.
    Invites,
    /// Drops a contact's invite this device keeps.
    DropInvite { invite: TeamInviteArgs },
}

/// What `TeamAppArgs::Cards` answers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamCards {
    /// Asks for the user, and automatic sessions that ended without a pull
    /// request: this mesh's own, never shown to teammates.
    pub needs_you: Vec<Card>,
    pub teammates: Vec<Card>,
}
