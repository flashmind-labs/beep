//! Live updates for a project: the node takes a repository's webhook
//! deliveries and tells the server which events came, so the project page
//! redraws within seconds instead of on its next read of the host.
//!
//! The server opens a `webhook/1` session and sends one `Watch`. The node
//! checks the user is an admin of the repository, then for GitHub runs
//! `gh webhook forward` into a listener on its loopback, and for Forgejo
//! and Gitea creates a repository webhook pointing at its listener on the
//! private network. It answers `Live` once deliveries can come, or
//! `Polling` saying why they cannot, and then one `Event` per delivery
//! whose signature checks out. Closing the stream stops the forward and
//! removes the webhook the node made.
//!
//! Like the rest of the node link these are compact msgpack, so a field is
//! added only at the end with `#[serde(default)]`.

use crate::SecretText;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr};

pub const PROTOCOL: &str = "webhook/1";

/// The longest event name the node passes on.
pub const MAX_EVENT: usize = 64;

/// What the server asks, once, as the session's first message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    pub host: WatchHost,
    /// `owner/name`.
    pub repo: String,
    /// The webhook's secret, made by the server for this webhook. Every
    /// delivery is checked against it.
    pub secret: SecretText,
    /// Webhooks Beep made on this repository before and did not remove, as
    /// when a node stopped without cleaning up. The node deletes these
    /// first, and no others.
    #[serde(default)]
    pub stale: Vec<u64>,
    /// Only remove `stale`, as for a removed project, and end the session.
    /// A node from before makes a webhook too, removed when the server
    /// closes.
    #[serde(default)]
    pub remove_only: bool,
}

/// Where the repository is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WatchHost {
    GitHub {
        /// The env var holding the token for this repository's owner when
        /// it is not the default account's (see `MachineCredentials`);
        /// `GH_TOKEN` otherwise.
        #[serde(default)]
        token_var: Option<String>,
    },
    /// A Forgejo or Gitea server, by the host name teacli knows it as.
    Forgejo { host: String },
}

/// What the node sends back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WatchReply {
    /// Deliveries can come now. `hook` is the id of the webhook the node
    /// made, which the server keeps until the node says it removed it.
    Live {
        #[serde(default)]
        hook: Option<u64>,
    },
    /// No live updates from this node; the stream ends after it.
    Polling {
        why: PollReason,
        /// What the tool said, for the log; never shown as is.
        #[serde(default)]
        detail: String,
    },
    /// One delivery whose signature checked out.
    Event {
        /// The host's event name, such as `pull_request` or `push`.
        event: String,
    },
    /// The webhook `hook` is gone: removed by the node, or no longer on
    /// the host.
    Removed { hook: u64 },
}

/// Why a node gives no live updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[non_exhaustive]
pub enum PollReason {
    /// The user is not an admin of the repository: only admins may add
    /// webhooks. Every node would answer the same.
    NotAdmin,
    /// `gh`, its webhook extension, or `teacli` is missing or signed out.
    NoTool,
    /// The node has no address the Forgejo server can reach it on: its
    /// `webhook_listen` setting is unset.
    NoAddress,
    /// The node runs as many watches as it takes.
    Busy,
    /// Anything else, such as the host refusing the webhook.
    Failed,
}

impl PollReason {
    /// Whether another node may do better: a reason about this node, not
    /// about the repository.
    pub fn node_specific(self) -> bool {
        !matches!(self, Self::NotAdmin)
    }
}
