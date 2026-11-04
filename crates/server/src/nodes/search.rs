//! Where a node looks for git checkouts, over `search/1` (see
//! `beep_proto::node::search`).

use super::Online;
use crate::protocol::node::search::{PROTOCOL, Reply, Request};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

const SEARCH_TIMEOUT: Duration = Duration::from_secs(10);

impl Online {
    /// Whether the node serves `search/1`.
    pub(crate) fn runs_search(&self) -> bool {
        self.offers(PROTOCOL)
    }

    /// The folder the node looks for checkouts in, as written.
    pub(crate) async fn search(&self) -> Result<String> {
        self.ask_search(Request::Get {}).await
    }

    /// Has the node look in `folder` (empty for home) and walk it again.
    pub(crate) async fn set_search(&self, folder: &str) -> Result<String> {
        self.ask_search(Request::Set {
            folder: folder.to_owned(),
        })
        .await
    }

    async fn ask_search(&self, request: Request) -> Result<String> {
        let (mut tx, mut rx) = self.open_stream(PROTOCOL).await?;
        tx.send(&request).await?;
        let reply = timeout(SEARCH_TIMEOUT, rx.recv::<Reply>())
            .await
            .context("the node took too long to answer")??
            .context("the node closed the stream without an answer")?;
        match reply {
            Reply::Folder { folder } => Ok(folder),
            Reply::Failed { reason } => bail!("{reason}"),
            other => bail!("the node answered {other:?}"),
        }
    }
}
