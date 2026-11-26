//! What `beep-node status` reports of the agents found here. Part of
//! `link::local_status::LocalStatus`, so its fields follow the wire rules there.

use serde::{Deserialize, Serialize};

/// A coding agent whose CLI the node found. Part of `LocalStatus`, so its
/// fields follow the wire rules there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub state: AgentState,
}

/// Whether the node can start an agent, and how its adapter stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentState {
    Ready,
    /// Ready, on the ACP adapter the node installed itself.
    OwnAdapter,
    /// Its CLI is here but not its adapter: the node is installing it.
    Installing,
    /// Installing its adapter failed; the node tries again later.
    Failed {
        reason: String,
    },
}
