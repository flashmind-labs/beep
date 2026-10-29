//! Files on a node (`files/1`), inside the folders it shares. Each call
//! opens its own stream: requests are few and do not depend on each other.

use super::Online;
use crate::protocol::node::files::{PROTOCOL, Reply, Request};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long one request may take, such as a large read on a slow disk.
const LIMIT: Duration = Duration::from_secs(30);

impl Online {
    /// Sends `request` to the node's files and returns its reply.
    pub async fn files(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!(
                "{} shares no folders; its owner lists them as `files = [\"/path\"]` in \
                 beep-node's node.toml and restarts it",
                self.name
            );
        }
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(request).await?;
        timeout(LIMIT, rx.recv::<Reply>())
            .await
            .context("the node stopped answering")??
            .context("the node closed the files session")
    }
}
