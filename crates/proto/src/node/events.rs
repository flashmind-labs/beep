//! Topic subscriptions and correlated commands on one bidirectional mesh stream.

use super::{agent, process, sign_in, sync, terminal};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "events/1";
pub const MAX_TOPICS: usize = 64;

mod git;
pub use git::{GitOutput, GitRead};

mod topic;
pub use topic::{MAX_TOPIC_BYTES, Topic, TopicError};

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
                S::Worktrees { folder } => Topic::keyed("files", folder),
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub id: u64,
    pub topic: Topic,
    pub read: Read,
}

/// A complete replacement for one projection. Reconnect starts with fresh snapshots.
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
        session: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_queries_only_accept_reads() {
        assert!(
            Read::Sessions(Box::new(agent::SessionsRequest::ListAll {}))
                .topic()
                .is_some()
        );
        assert!(
            Read::Sessions(Box::new(agent::SessionsRequest::WriteFile {
                root: "/srv/project".into(),
                path: "file".into(),
                offset: 0,
                size: 0,
                data: vec![]
            }))
            .topic()
            .is_none()
        );
        assert!(
            Read::Process(process::Request::Start {
                command: "false".into(),
                cwd: None
            })
            .topic()
            .is_none()
        );
        assert!(
            Read::Sessions(Box::new(agent::SessionsRequest::FollowPresence {}))
                .topic()
                .is_none()
        );
    }

    #[test]
    fn snapshot_subscriptions_roundtrip_compact_messagepack() {
        let read = Read::Sessions(Box::new(agent::SessionsRequest::Folder {
            root: "/srv/project".into(),
            path: String::new(),
            ignored: false,
            depth: 1,
        }));
        let request = Request::Watch {
            id: 1,
            topics: vec![Topic::parse("files.org.repo").unwrap()],
            queries: vec![Query {
                id: 2,
                topic: read.topic().unwrap(),
                read,
            }],
        };
        let bytes = rmp_serde::to_vec(&request).unwrap();
        assert_eq!(
            crate::lenient::from_slice::<Request>(&bytes).unwrap(),
            request
        );
        let reply = Reply::Snapshot(Box::new(Snapshot {
            topic: Topic::parse("files.org.repo").unwrap(),
            query: Some(2),
            revision: 1,
            value: Value::Missing,
        }));
        assert_eq!(
            crate::lenient::from_slice::<Reply>(&rmp_serde::to_vec(&reply).unwrap()).unwrap(),
            reply
        );
    }

    #[test]
    fn subscriptions_match_a_resource_a_namespace_or_everything() {
        let event = Topic::keyed("files", "/srv/project");
        assert!(event.matches(&event));
        assert!(Topic::new("files").matches(&event));
        assert!(Topic::new("*").matches(&event));
        assert!(!Topic::keyed("files", "/srv/other").matches(&event));
        assert!(!Topic::new("reviews").matches(&event));
        assert!(event.matches(&Topic::new("files")));
        assert!(!Topic::keyed("files", "bad\0path").valid());
    }
}
