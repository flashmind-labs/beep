//! The control stream's messages, and how a session stream opens.

use super::{EnvReport, StatsReport, harness, traffic};
use crate::SecretText;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The server link. Messages are compact msgpack, read by position.
pub const NODE_ALPN: &[u8] = b"beep-node/2";
/// The server link of released nodes, whose messages are msgpack maps. A node
/// offers it after `NODE_ALPN` and a server accepts both, so either side may
/// be updated first.
pub const NODE_ALPN_NAMED: &[u8] = b"beep-node/1";

/// The code the server closes a node's connection with when it refuses the
/// hello's setup token; the reason says why, in words for the user.
pub const CLOSE_SETUP_REFUSED: u32 = 400;

/// The first message on the control stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeHello {
    /// The daemon's version, such as "0.1.0".
    pub version: String,
    /// What the user calls the machine, such as "dev server".
    pub name: String,
    /// The session protocols this node serves, such as `shell/1` or
    /// `claude/1`. A session stream opens with one of these names; a node
    /// only offers a tool it has.
    #[serde(default)]
    pub offers: Vec<String>,
    /// The folder Beep keeps the user's files in and runs its code in, when
    /// this node is the user's own environment (such as deploy/onprem's
    /// node container). It is also the first folder `files/1` shares.
    #[serde(default)]
    pub workspace: Option<String>,
    /// A setup token to claim before anything else (`beep-node setup`):
    /// the server links this node's key to the account that made the token,
    /// says so with `Down::Linked`, then welcomes it as any linked node. A
    /// server from before tokens closes the connection with "not linked".
    #[serde(default)]
    pub setup: Option<NodeSetup>,
    /// The node takes `Down::Credentials`. A node from before it never
    /// gets the user's credentials.
    #[serde(default)]
    pub credentials: bool,
    /// The node makes the branch or worktree `agent::Request::Start`'s
    /// `checkout` asks for. A node from before it ignores that field, so
    /// the server refuses a checkout plan for it.
    #[serde(default)]
    pub checkouts: bool,
    /// The node takes `Down::AgentSettings`. A node from before it starts
    /// its harnesses as it finds them.
    #[serde(default)]
    pub agent_settings: bool,
    /// The node makes `agent::CheckoutPlan::NamedWorktree` and
    /// `BranchWorktree`. A node from before them cannot read a start that
    /// asks for one, so the server refuses those plans for it.
    #[serde(default)]
    pub branch_checkouts: bool,
}

/// What `NodeHello::setup` carries.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSetup {
    /// The token as `setup::SetupToken` writes it.
    pub token: String,
    /// The name the user gave on the machine, which wins over the token's.
    /// Without either, the machine is called by the hello's `name`.
    #[serde(default)]
    pub name: Option<String>,
}

/// Never shows the token.
impl fmt::Debug for NodeSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeSetup")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// What a node sends on the control stream after its hello.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
// Sent a few times an hour, never kept in bulk: the size does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Up {
    /// What the machine has. Sent after the hello and again when it changes.
    Env(EnvReport),
    /// How much disk and memory the machine has free now. Sent after the
    /// first `Env` and about once a minute after. A server from before it
    /// skips it.
    Stats(StatsReport),
    /// The bytes the node moved over iroh, about once a minute. A server
    /// from before it skips it.
    Traffic(traffic::TrafficReport),
}

/// What the server sends on the control stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Down {
    Welcome {
        /// The server takes `tools/1` streams from the node. A server from
        /// before them leaves it out, and a node from before them skips it.
        #[serde(default)]
        tools: bool,
    },
    /// The node's setup token was claimed: its key now signs into
    /// `account` (the account's name) as the machine `name`. Sent before the
    /// welcome, only to a node whose hello carried `setup`.
    Linked { account: String, name: String },
    /// The user's credentials meant for their machines, sent after the
    /// welcome and again whenever they change, each time in full. Only to
    /// a node whose hello says it takes them. The node keeps them in
    /// memory.
    Credentials(MachineCredentials),
    /// The user's settings for the coding agent harnesses on this machine,
    /// sent after the welcome and again whenever they change, each time in
    /// full. Only to a node whose hello says it takes them.
    AgentSettings(harness::AgentSettings),
}

/// What a node uses on the user's behalf.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineCredentials {
    /// Env vars every process the node starts gets, such as `GH_TOKEN`.
    pub env: Vec<EnvSecret>,
    /// The Forgejo and Gitea servers teacli signs in to.
    pub forgejo: Vec<ForgejoLogin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvSecret {
    pub name: String,
    pub value: SecretText,
}

/// One Forgejo or Gitea server and the token for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgejoLogin {
    /// Its base URL, such as `https://git.example.com`, without a trailing
    /// slash.
    pub url: String,
    pub token: SecretText,
}

/// The first message on a session stream: the protocol it speaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub protocol: String,
}

/// The node's answer to `Open`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Opened {
    Ready {},
    Refused { reason: String },
}
