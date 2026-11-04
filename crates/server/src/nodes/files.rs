//! Files in a node's shared folders (`files/1`), one stream per call.

use super::{Online, Repeat, retry::Unsent};
use crate::protocol::node::files::{PROTOCOL, Reply, Request};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long one request may take, such as a large read on a slow disk.
const LIMIT: Duration = Duration::from_secs(30);

impl Online {
    /// Sends `request` to the node's files and returns its reply. When the
    /// link drops, it goes again once the node is back, unless it appends.
    pub async fn files(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!(
                "{} shares no folders; its owner lists them as `files = [\"/path\"]` in \
                 beep-node's node.toml and restarts it",
                self.name
            );
        }
        let repeat = Repeat::when(!matches!(request, Request::Write { append: true, .. }));
        self.retrying(repeat, |on| async move { on.files_once(request).await })
            .await
    }

    async fn files_once(&self, request: &Request) -> Result<Reply> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(request).await.map_err(Unsent)?;
        timeout(LIMIT, rx.recv::<Reply>())
            .await
            .context("the node stopped answering")??
            .context("the node closed the files session")
    }
}
