//! Asking a node about its coding agent harnesses.

use super::Online;
use crate::protocol::node::harness::HarnessReport;
use anyhow::Result;
use beep_mesh::machine::sessions;

impl Online {
    /// What the node finds of each harness; `None` from a node too old to say.
    pub(crate) async fn harnesses(&self, refresh: bool) -> Result<Option<HarnessReport>> {
        Ok(sessions::harnesses(self, refresh).await?)
    }

    /// Installs or updates `agent` on the node; `None` from a node too old to.
    pub(crate) async fn update_harness(&self, agent: &str) -> Result<Option<HarnessReport>> {
        Ok(sessions::update_harness(self, agent).await?)
    }
}
