//! The traffic of the user's machines, as they report it.

use super::User;
use crate::protocol::{MachineTraffic, node::traffic::TrafficReport};
use anyhow::Result;
use iroh::EndpointId;

impl User {
    /// Adds what the machine `endpoint` moved since its last report.
    pub async fn save_node_traffic(
        &self,
        endpoint: &EndpointId,
        report: &TrafficReport,
    ) -> Result<()> {
        self.store
            .save_node_traffic(&self.key, endpoint, report)
            .await
    }

    /// The lifetime totals of the user's machines.
    pub(crate) async fn machine_traffic(&self) -> Result<Vec<MachineTraffic>> {
        self.store.machine_traffic(&self.key).await
    }
}
