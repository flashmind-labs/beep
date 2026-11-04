//! A node's recent log, over `logs/1` (see `beep_proto::node::logs`).

use super::Online;
use crate::protocol::node::logs::{PROTOCOL, Reply, Request};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long the node has to read its log.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

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
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(&Request { lines }).await?;
        let reply = timeout(READ_TIMEOUT, rx.recv::<Reply>())
            .await
            .context("the node took too long to read its log")??
            .context("the node closed the stream without its log")?;
        match reply {
            Reply::Log { text } => Ok(text),
            Reply::Failed { reason } => bail!(reason),
            other => bail!("the node answered {other:?}"),
        }
    }
}
