//! Background commands on a node (`process/1`). Each call opens its own
//! stream; the tasks belong to the node and outlive it.

use super::Online;
use crate::protocol::node::process::{PROTOCOL, Reply, Request};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long one request may take; a stop waits for the group to end.
const LIMIT: Duration = Duration::from_secs(20);

impl Online {
    /// Sends `request` to the node's background tasks and returns its reply.
    pub async fn tasks(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!(
                "{} runs no background commands; it needs a newer beep-node",
                self.name
            );
        }
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(request).await?;
        timeout(LIMIT, rx.recv::<Reply>())
            .await
            .context("the node stopped answering")??
            .context("the node closed the process session")
    }
}
