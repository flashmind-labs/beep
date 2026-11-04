//! Newer `gh`, `teacli` and `flsh` for the machines: the newest release of
//! each, looked up every few hours, and installing it on a node over
//! `tool-update/1` (see `beep_proto::node::tool_update`).

use super::Online;
use crate::protocol::node::tool_update::{FLSH, PROTOCOL, Tool};
use anyhow::Result;
use beep_mesh::{PeerClient, machine};

impl Online {
    /// Whether it installs newer tools over `tool-update/1`.
    pub(crate) fn installs_tools(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// Whether it installs newer releases of `tool`.
    pub(crate) fn installs(&self, tool: Tool) -> bool {
        self.installs_tools() && (tool != Tool::Flsh || self.offers(FLSH))
    }

    /// Has the node install release `version` of `tool`. `Err` holds the
    /// node's reason, in words for the user.
    pub(crate) async fn install_tool(&self, tool: Tool, version: &str) -> Result<()> {
        let client = PeerClient::new(self.connection.clone());
        Ok(machine::install_tool(&client, tool, version).await?)
    }
}
