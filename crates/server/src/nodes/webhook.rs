//! Opens a node's `webhook/1` session, which forwards one repository's
//! webhook for live project updates (see `crate::repo_watch::forward`).

use super::Online;
use crate::protocol::node::{
    LinkRx, LinkTx,
    webhook::{PROTOCOL, Watch},
};
use anyhow::Result;
use iroh::EndpointId;

impl Online {
    /// Whether the node forwards webhooks.
    pub(crate) fn forwards_webhooks(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// The node's key.
    pub(crate) fn endpoint(&self) -> EndpointId {
        self.connection.remote_id()
    }

    /// Asks the node to watch a repository: the session lasts as long as
    /// the returned stream.
    pub(crate) async fn watch_webhook(&self, watch: &Watch) -> Result<(LinkTx, LinkRx)> {
        let (mut tx, rx) = self.open_stream(PROTOCOL).await?;
        tx.send(watch).await?;
        Ok((tx, rx))
    }
}
