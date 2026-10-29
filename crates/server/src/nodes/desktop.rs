//! Driving a node's desktop (`desktop/1`). Each node keeps one desktop
//! stream, opened when first needed, so the element indexes of one tree and
//! the scale of one screenshot hold for the commands after it.

use super::Online;
use crate::{
    AsyncMutex,
    protocol::node::{
        LinkRx, LinkTx,
        desktop::{PROTOCOL, Reply, Request},
    },
};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How long one request may take: a screenshot, a deep tree or an app
/// that opens slowly.
const LIMIT: Duration = Duration::from_secs(30);

/// The node's desktop stream, while one is open.
pub type DesktopSlot = AsyncMutex<Option<DesktopLink>>;

pub struct DesktopLink {
    tx: LinkTx,
    rx: LinkRx,
}

impl Online {
    /// Sends `request` to the node's desktop, after any request already
    /// running, and returns its reply.
    pub async fn desktop(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!("{} does not offer desktop control", self.name);
        }
        let mut slot = self.desktop.lock().await;
        // Taken out while the request runs, so one cut short drops the
        // stream instead of leaving its reply for the next request to read.
        let mut desktop = match slot.take() {
            Some(desktop) => desktop,
            None => {
                let (tx, rx) = self.open_stream(PROTOCOL).await?;
                DesktopLink { tx, rx }
            }
        };
        desktop.tx.send(request).await?;
        let reply = timeout(LIMIT, desktop.rx.recv::<Reply>())
            .await
            .context("the node stopped answering")??
            .context("the node closed the desktop session")?;
        *slot = Some(desktop);
        Ok(reply)
    }
}
