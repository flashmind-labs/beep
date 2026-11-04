//! Where a node looks for git checkouts, over `search/1` (see
//! `beep_proto::node::search`).

use super::Online;
use crate::protocol::node::search::PROTOCOL;
use anyhow::Result;
use beep_mesh::{PeerClient, machine};

impl Online {
    /// Whether the node serves `search/1`.
    pub(crate) fn runs_search(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// The folder the node looks for checkouts in, as written.
    pub(crate) async fn search(&self) -> Result<String> {
        Ok(machine::search(&PeerClient::new(self.connection.clone())).await?)
    }

    /// Has the node look in `folder` (empty for home) and walk it again.
    pub(crate) async fn set_search(&self, folder: &str) -> Result<String> {
        Ok(machine::set_search(&PeerClient::new(self.connection.clone()), folder).await?)
    }
}
