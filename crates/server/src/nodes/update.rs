//! Updating a node's beep-node from Beep, over `update/1` (see
//! `beep_proto::node::update`).

use super::Online;
use crate::protocol::node::{
    agent::{SESSIONS_PROTOCOL, Status},
    update::PROTOCOL,
};
use anyhow::Result;
use beep_mesh::{PeerClient, machine};

pub(crate) use beep_mesh::machine::Updated;

impl Online {
    /// Whether it can be updated over `update/1`. The Mac app's node and
    /// nodes from before it cannot.
    pub(crate) fn updatable(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// The agent sessions the node runs that have not ended, which its
    /// restart waits for.
    pub(crate) async fn running_agent_sessions(&self) -> Result<u32> {
        if !self.offers(SESSIONS_PROTOCOL) {
            return Ok(0);
        }
        let sessions = self.agent_sessions().await?;
        let running = sessions
            .iter()
            .filter(|session| session.state.status != Status::Ended)
            .count();
        Ok(u32::try_from(running).unwrap_or(u32::MAX))
    }

    /// Has the node install the newest release and restart, once its
    /// agent sessions are idle or, with `now`, at once. `Err` holds the
    /// node's reason, in words for the user.
    pub(crate) async fn update(&self, now: bool) -> Result<Updated> {
        let client = PeerClient::new(self.connection.clone());
        Ok(machine::update_node(&client, now).await?)
    }
}
