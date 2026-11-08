//! Topic subscriptions and correlated commands on one bidirectional mesh stream.

use super::{agent, process};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL: &str = "events/1";
pub const MAX_TOPICS: usize = 64;

/// A namespace and optional resource key. An absent key matches the whole namespace.
/// `*` matches every namespace. Plugins may use their own namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Topic {
    pub name: String,
    pub key: Option<String>,
}

impl Topic {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            key: None,
        }
    }
    pub fn keyed(name: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            key: Some(key.into()),
        }
    }
    pub fn matches(&self, topic: &Self) -> bool {
        (self.name == "*" || self.name == topic.name)
            && self
                .key
                .as_ref()
                .is_none_or(|key| topic.key.is_none() || topic.key.as_ref() == Some(key))
    }
    pub fn valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.len() <= 128
            && (self.name == "*"
                || self
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-/".contains(&c)))
            && self
                .key
                .as_ref()
                .is_none_or(|key| !key.is_empty() && key.len() <= 4096 && !key.contains('\0'))
    }
}

/// The epoch changes when the node restarts or its history is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventId {
    pub epoch: Uuid,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// A missing resource key invalidates the namespace for every matching subscriber.
pub struct Event {
    pub topic: Topic,
    pub id: EventId,
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
        after: Option<EventId>,
    },
    /// Executed once on this connection. A lost reply has an unknown outcome.
    Call {
        id: u64,
        operation: Box<Operation>,
    },
    Ping {
        id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    Ready,
    Watching {
        id: u64,
    },
    Changed(Event),
    /// Initial connection, a changed subscription, or a slow reader: read current state.
    Resync {
        cursor: EventId,
    },
    /// All events through this cursor were considered, including unselected topics.
    CaughtUp {
        cursor: EventId,
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
