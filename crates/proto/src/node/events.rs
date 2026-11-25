//! Topic subscriptions and correlated commands on one bidirectional mesh stream.

use super::{agent, device, process, sign_in, sync, terminal, tool_update, tools};
use crate::SessionId;
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "events/1";
pub const MAX_TOPICS: usize = 64;
/// The topic of a team's cards, keyed by the project's name: `Value::TeamCards`.
pub const TEAM_CARDS_TOPIC: &str = "team-cards";
/// The topic of how each move to a node stands, as `SessionsReply::Moves`
/// answers it. Only a topic that names it gets it, never `*`.
pub const MOVE_PROGRESS_TOPIC: &str = "move-progress";
/// The topic of the contacts' team invites waiting for a desktop on this
/// node: `Value::TeamInvites`.
pub const TEAM_INVITES_TOPIC: &str = "team-invites";

mod git;
pub use git::{GitOutput, GitRead};

mod topic;
pub use topic::{Topic, TopicError};

pub const MAX_QUERIES: usize = 128;

/// A projection of current resource state. Mutating and following requests are rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Read {
    Sessions(Box<agent::SessionsRequest>),
    Process(process::Request),
    SignIn { tool: String },
    TerminalList,
    SyncList,
    Git { root: String, query: GitRead },
}

impl Read {
    /// The source that owns this projection, also the read-only allowlist.
    pub fn topic(&self) -> Option<Topic> {
        use agent::SessionsRequest as S;
        Some(match self {
            Self::Sessions(request) => match request.as_ref() {
                S::List {}
                | S::ListAll {}
                | S::External { .. }
                | S::Folders { .. }
                | S::InFolders { .. } => Topic::new("agents"),
                S::Folder { root, .. } | S::ReadFile { root, .. } => Topic::keyed("files", root),
                S::Diff { ask } => Topic::keyed("files", &ask.root),
                S::Worktrees { folder, .. } => Topic::keyed("files", folder),
                S::Present { .. } => Topic::new("files"),
                S::Live { session } | S::Commands { session } => Topic::keyed("agent", session),
                S::MoveProgress { session } => Topic::keyed("moves", session),
                S::Copies {} => Topic::new("moves"),
                S::Review { .. } | S::Reviews { .. } => Topic::new("reviews"),
                S::Processes { .. } => Topic::new("processes"),
                // Transcript paging and media are finite reads. The conversation stream
                // owns live transcript cursors and suffix replacements.
                _ => return None,
            },
            Self::Process(process::Request::List {} | process::Request::Status { .. }) => {
                Topic::new("processes")
            }
            Self::Process(_) => return None,
            Self::SignIn { tool } => Topic::keyed("sign-in", tool),
            Self::TerminalList => Topic::new("terminals"),
            Self::SyncList => Topic::new("sync"),
            Self::Git { root, .. } => Topic::keyed("files", root),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// The resource has no current state, including an unbound project topic.
    Missing,
    /// Exact current resources in the snapshot topic scope. Absent resources are removed.
    Resources(Vec<Topic>),
    Git(GitOutput),
    Sessions(Box<agent::SessionsReply>),
    Process(process::Reply),
    SignIn(sign_in::State),
    Terminals(terminal::Reply),
    Sync(Box<sync::Reply>),
    /// A plugin's compact MessagePack state, published under its topic.
    Plugin(#[serde(with = "serde_bytes")] Vec<u8>),
    /// Every emulator, phone and simulator on the node, under `device-list`.
    /// Only a topic that names it gets it, never `*`.
    Devices(Vec<device::Device>),
    /// The tools the node has found a newer release of, under `tool-updates`.
    /// `Missing` until the node's first check ends. Only a topic that names
    /// it gets it, never `*`.
    ToolUpdates(Vec<tool_update::Available>),
    /// A team's Needs you and teammates' cards, as `TeamAppArgs::Cards`
    /// answers them, under `team-cards` keyed by the project's name. Only the
    /// machine that acts for the team has it: any other answers `Missing`.
    /// Only a topic that names it gets it, never `*`. A node from before it
    /// answers a `SnapshotError` for the topic.
    TeamCards(tools::TeamCards),
    /// The invites waiting for a desktop on this node, as `TeamAppArgs::Invites`
    /// answers them, under the unkeyed `team-invites`. Only a topic that names
    /// it gets it, never `*`. A node from before it answers a `SnapshotError`
    /// for the topic.
    TeamInvites(Vec<tools::TeamInviteArgs>),
    /// What changed in the `agents` list since the snapshot or delta
    /// `after` names, for a follower that asked for deltas. Never in a
    /// snapshot of any other topic or query.
    SessionsDelta(Box<SessionsDelta>),
}

/// The rows of the `agents` list that changed, by session id, and the ids
/// that left it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionsDelta {
    /// The revision of the snapshot or delta of this topic sent just before.
    /// A follower that does not hold that one missed a message and needs
    /// the whole list again.
    pub after: u64,
    pub changed: Vec<agent::SessionInfo>,
    pub removed: Vec<SessionId>,
}

impl SessionsDelta {
    /// What takes `before` to `now`.
    pub fn between(before: &[agent::SessionInfo], now: &[agent::SessionInfo], after: u64) -> Self {
        let held: std::collections::HashMap<_, _> =
            before.iter().map(|info| (&info.id, info)).collect();
        let kept: std::collections::HashSet<_> = now.iter().map(|info| &info.id).collect();

        Self {
            after,
            changed: now
                .iter()
                .filter(|info| held.get(&info.id) != Some(info))
                .cloned()
                .collect(),
            removed: before
                .iter()
                .filter(|info| !kept.contains(&info.id))
                .map(|info| info.id.clone())
                .collect(),
        }
    }

    /// Applies it to the list it was made against, oldest session first.
    pub fn apply(self, sessions: &mut Vec<agent::SessionInfo>) {
        sessions.retain(|info| !self.removed.contains(&info.id));

        for row in self.changed {
            match sessions.iter_mut().find(|info| info.id == row.id) {
                Some(held) => *held = row,
                None => sessions.push(row),
            }
        }

        sessions.sort_by_key(|info| info.started_at);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub id: u64,
    pub topic: Topic,
    pub read: Read,
}

/// A complete replacement for one projection, but for a `Value::SessionsDelta`.
/// Reconnect starts with fresh snapshots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub topic: Topic,
    pub query: Option<u64>,
    /// Delivery order within this stream, never a replay cursor.
    pub revision: u64,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    Agent {
        agent: String,
        session: SessionId,
        request: agent::Request,
    },
    Sessions(agent::SessionsRequest),
    Process(process::Request),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CallReply {
    Agent { info: Box<agent::SessionInfo> },
    Sessions(Box<agent::SessionsReply>),
    Process(process::Reply),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// Replaces the subscription set. Acknowledged after watchers are installed.
    Watch {
        id: u64,
        topics: Vec<Topic>,
        queries: Vec<Query>,
    },
    /// Executed once on this connection. A lost reply has an unknown outcome.
    Call {
        id: u64,
        operation: Box<Operation>,
    },
    Ping {
        id: u64,
    },
    /// Request fresh snapshots of all active projections without replacing their sources.
    Refresh {
        id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    Ready,
    Watching {
        id: u64,
    },
    Snapshot(Box<Snapshot>),
    SnapshotError {
        query: Option<u64>,
        topic: Topic,
        reason: String,
    },
    Result {
        id: u64,
        reply: Box<CallReply>,
    },
    Error {
        id: u64,
        reason: String,
    },
    Pong {
        id: u64,
    },
}

/// The `agents` list as a follower of deltas holds it, to apply them to.
#[derive(Debug, Default)]
pub struct HeldSessions {
    /// The revision of the last message of the list, 0 before the first.
    revision: u64,
    sessions: Vec<agent::SessionInfo>,
}

impl HeldSessions {
    /// `snapshot` as a snapshot of the whole list: a delta applied to what
    /// is held, any other snapshot as it came. `None` for a delta that does
    /// not follow the last message held, which means one was missed: the
    /// follower has to watch again for the whole list.
    pub fn hear(&mut self, mut snapshot: Snapshot) -> Option<Snapshot> {
        if snapshot.topic.name() != "agents" || snapshot.query.is_some() {
            return Some(snapshot);
        }

        if let Value::SessionsDelta(delta) = &mut snapshot.value {
            if self.revision == 0 || delta.after != self.revision {
                return None;
            }

            std::mem::take(delta).apply(&mut self.sessions);
            snapshot.value = Value::Sessions(Box::new(agent::SessionsReply::Sessions {
                sessions: self.sessions.clone(),
            }));
            self.revision = snapshot.revision;
            return Some(snapshot);
        }

        match &snapshot.value {
            Value::Sessions(reply) => match reply.as_ref() {
                agent::SessionsReply::Sessions { sessions } => {
                    self.sessions.clone_from(sessions);
                    self.revision = snapshot.revision;
                }
                _ => self.revision = 0,
            },
            _ => self.revision = 0,
        }

        Some(snapshot)
    }
}

/// A request as this build reads it. A `Call` whose operation is newer than
/// this build keeps its id, so the stream answers it with an error and
/// carries on instead of ending.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    Request(Request),
    Unread { id: u64, reason: String },
}

/// Only the id of a `Call`, read when its operation is not.
#[derive(Deserialize)]
enum CallId {
    Call {
        id: u64,
        #[allow(dead_code)]
        operation: serde::de::IgnoredAny,
    },
}

impl Incoming {
    pub fn read(frame: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        let error = match crate::lenient::from_slice(frame) {
            Ok(request) => return Ok(Self::Request(request)),
            Err(error) => error,
        };

        match crate::lenient::from_slice(frame) {
            Ok(CallId::Call { id, .. }) => Ok(Self::Unread {
                id,
                reason: format!("this node cannot read the call: {error}"),
            }),
            Err(_) => Err(error),
        }
    }
}

// The inbound events/1 protocol remains available to released apps.
impl From<super::subscriptions::Reply> for Reply {
    fn from(reply: super::subscriptions::Reply) -> Self {
        use super::subscriptions::Reply as Subscription;
        match reply {
            Subscription::Ready => Self::Ready,
            Subscription::Watching { id } => Self::Watching { id },
            Subscription::Snapshot(snapshot) => Self::Snapshot(snapshot),
            Subscription::SnapshotError {
                query,
                topic,
                reason,
            } => Self::SnapshotError {
                query,
                topic,
                reason,
            },
            Subscription::Error { id, reason } => Self::Error { id, reason },
            Subscription::Pong { id } => Self::Pong { id },
        }
    }
}

#[cfg(test)]
#[path = "../tests/node_events.rs"]
mod tests;
