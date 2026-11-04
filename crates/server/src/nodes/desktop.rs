//! Driving a node's desktop (`desktop/1`) on one stream, so tree indexes and scale carry over.

use super::{NODE_BACK, Online, Repeat, retry::Unsent};
use crate::{
    AsyncMutex,
    protocol::node::{
        LinkRx, LinkTx,
        desktop::{PROTOCOL, Reply, Request},
    },
};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::{Instant, timeout};

/// How long one request may take.
const LIMIT: Duration = Duration::from_secs(30);

/// The node's desktop stream, while one is open.
pub type DesktopSlot = AsyncMutex<Option<DesktopLink>>;

pub struct DesktopLink {
    tx: LinkTx,
    rx: LinkRx,
}

impl Online {
    /// Sends `request` to the node's desktop, after any already running.
    /// When the link drops, a look goes again once the node is back, and
    /// an action does if the node never got it.
    pub async fn desktop(&self, request: &Request) -> Result<Reply> {
        if !self.offers(PROTOCOL) {
            bail!("{} does not offer desktop control", self.name);
        }
        let looks = matches!(request, Request::Screenshot { .. } | Request::Tree { .. });
        let until = Instant::now() + NODE_BACK;
        let mut on = self.clone();
        loop {
            let error = match on.desktop_once(request).await {
                Ok(reply) => return Ok(reply),
                Err(error) => error,
            };
            match on.again(&error, Repeat::when(looks), until).await {
                Some(back) => on = back,
                None => return Err(error),
            }
        }
    }

    async fn desktop_once(&self, request: &Request) -> Result<Reply> {
        let mut slot = self.desktop.lock().await;
        // Taken out while running, so a cancelled request drops the stream instead of its reply.
        let mut desktop = match slot.take() {
            Some(desktop) => desktop,
            None => {
                let (tx, rx) = self.open_stream(PROTOCOL).await?;
                DesktopLink { tx, rx }
            }
        };
        desktop.tx.send(request).await.map_err(Unsent)?;
        let reply = timeout(LIMIT, desktop.rx.recv::<Reply>())
            .await
            .context("the node stopped answering")??
            .context("the node closed the desktop session")?;
        *slot = Some(desktop);
        Ok(reply)
    }
}
