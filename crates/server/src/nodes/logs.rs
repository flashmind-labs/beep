//! A node's recent log, over `logs/1` (see `beep_proto::node::logs`).

use super::Online;
use crate::protocol::node::logs::PROTOCOL;
use anyhow::{Result, bail};
use beep_mesh::{PeerClient, machine};

impl Online {
    /// The last `lines` lines of the node's log. `Err` says why not, in words
    /// for the user.
    pub(crate) async fn log(&self, lines: u32) -> Result<String> {
        if !self.offers(PROTOCOL) {
            bail!(
                "beep-node on {} is too old to send its log. Update it.",
                self.name
            );
        }
        Ok(machine::log(&PeerClient::new(self.connection.clone()), lines).await?)
    }
}
