//! Topic subscriptions and correlated commands on one bidirectional mesh stream.

use super::{agent, process};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL: &str = "events/1";
pub const MAX_TOPICS: usize = 64;

mod topic;
pub use topic::{MAX_TOPIC_BYTES, Topic, TopicError};

/// The epoch changes when the node restarts or its history is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventId {
    pub epoch: Uuid,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// A wildcard topic invalidates its scope for every matching subscriber.
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
