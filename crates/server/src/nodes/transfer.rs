//! Sending a file or folder from one node to another (`transfer/1`). The
//! server brokers only: the source offers it to the target's key, the
//! target fetches it from the source directly, and the offer closes when
//! the fetch is done. The bytes never pass through the server.

use super::Online;
use crate::protocol::node::{
    LinkRx, LinkTx,
    transfer::{Content, PROTOCOL, Reply, Request, Stage},
};
use anyhow::{Context, Result, anyhow, bail};
use iroh::{EndpointAddr, EndpointId};
use std::time::Duration;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

/// The longest a source may go quiet while hashing, and a target while
/// fetching. Both report about every second while bytes move, so only a
/// transfer that stopped moving gets this far.
const OFFER_IDLE: Duration = Duration::from_secs(10 * 60);
const FETCH_IDLE: Duration = Duration::from_secs(10 * 60);
/// How long a target has to clean up after a cancel.
const CANCEL_LIMIT: Duration = Duration::from_secs(30);

/// Bytes done of a total, as a node reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub stage: Stage,
}

/// What a source offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Offered {
    pub content: Content,
    pub from: EndpointAddr,
    pub name: String,
    pub size: u64,
    pub files: u64,
    /// Symlinks and special files left out of a folder.
    pub skipped: u64,
    /// A file that is executable at the source.
    pub executable: bool,
}

/// An offer the source keeps open; the target may fetch it until this
/// drops.
pub struct Offer {
    pub offered: Offered,
    tx: LinkTx,
    rx: LinkRx,
}

impl Offer {
    /// Closes the offer and waits until the source dropped its grant.
    /// Dropping it closes it too, without waiting.
    pub async fn close(mut self) -> Result<()> {
        self.tx.send(&Request::Cancel {}).await?;
        let closed = timeout(CANCEL_LIMIT, async {
            while self.rx.recv::<Reply>().await?.is_some() {}
            anyhow::Ok(())
        });
        closed.await.context("the source did not close the offer")?
    }
}

/// Where a fetch landed on the target.
#[derive(Debug, Clone, PartialEq)]
pub struct Landed {
    pub path: String,
    pub size: u64,
    pub files: u64,
}

impl Online {
    async fn open_transfer(&self) -> Result<(LinkTx, LinkRx)> {
        if !self.offers(PROTOCOL) {
            bail!(
                "{} cannot send or take transfers; it needs a newer beep-node and shared folders",
                self.name
            );
        }
        self.open_stream(PROTOCOL).await
    }

    /// Asks this node to offer `path`, a file or folder in its shared
    /// folders, to the node `peer`.
    pub async fn offer(
        &self,
        path: &str,
        peer: EndpointId,
        progress: &mut (dyn FnMut(Progress) + Send),
    ) -> Result<Offer> {
        let (mut tx, mut rx) = self.open_transfer().await?;
        tx.send(&Request::Offer {
            path: path.to_owned(),
            peer,
        })
        .await?;
        loop {
            let reply = timeout(OFFER_IDLE, rx.recv::<Reply>())
                .await
                .with_context(|| format!("{} stopped answering while hashing", self.name))??
                .with_context(|| format!("{} closed the offer", self.name))?;
            match reply {
                Reply::Progress { done, total, stage } => {
                    progress(Progress { done, total, stage });
                }
                Reply::Offered {
                    content,
                    from,
                    name,
                    size,
                    files,
                    skipped,
                    executable,
                } => {
                    return Ok(Offer {
                        offered: Offered {
                            content,
                            from,
                            name,
                            size,
                            files,
                            skipped,
                            executable,
                        },
                        tx,
                        rx,
                    });
                }
                Reply::Error { message } => bail!("{}: {message}", self.name),
                other => bail!("{} answered {other:?}", self.name),
            }
        }
    }

    /// Asks this node to fetch `offered` into `folder`, an absolute folder
    /// in its shared folders. When `cancel` fires the node stops and
    /// removes what it wrote before this returns.
    pub async fn fetch(
        &self,
        offered: &Offered,
        folder: &str,
        progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<Landed> {
        let (mut tx, mut rx) = self.open_transfer().await?;
        tx.send(&Request::Fetch {
            from: offered.from.clone(),
            content: offered.content,
            name: offered.name.clone(),
            size: offered.size,
            folder: folder.to_owned(),
            executable: offered.executable,
        })
        .await?;
        let mut cancelled = false;
        loop {
            let limit = if cancelled { CANCEL_LIMIT } else { FETCH_IDLE };
            let reply = tokio::select! {
                () = cancel.cancelled(), if !cancelled => {
                    cancelled = true;
                    tx.send(&Request::Cancel {}).await?;
                    continue;
                }
                reply = timeout(limit, rx.recv::<Reply>()) => reply,
            };
            let reply = reply
                .with_context(|| format!("{} stopped answering", self.name))??
                .with_context(|| format!("{} closed the transfer", self.name))?;
            match reply {
                Reply::Progress { done, total, stage } => {
                    progress(Progress { done, total, stage });
                }
                Reply::Done { path, size, files } => return Ok(Landed { path, size, files }),
                Reply::Cancelled {} => return Err(anyhow!("the transfer was cancelled")),
                Reply::Error { message } => bail!("{}: {message}", self.name),
                other => bail!("{} answered {other:?}", self.name),
            }
        }
    }
}
