//! `logs/1` (see `beep_proto::node::logs`): the node's recent log, asked by
//! the server.

use super::recent;
use crate::link::{Protocol, Session};
use beep_proto::node::{
    LinkRx, LinkTx,
    logs::{PROTOCOL, Reply, Request},
};
use std::path::PathBuf;
use tokio::task::spawn_blocking;

/// Serves `logs/1` from the service's log, or from a log file of the host's.
pub struct Logs {
    file: Option<PathBuf>,
}

impl Logs {
    /// The log `beep-node logs` shows.
    pub fn service() -> Self {
        Self { file: None }
    }

    /// The log file at `path`, for a host that keeps its own, as the Mac app.
    pub fn file(path: PathBuf) -> Self {
        Self { file: Some(path) }
    }
}

impl Protocol for Logs {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        let file = self.file.clone();
        Box::pin(async move {
            let Some(Request { lines }) = rx.recv().await? else {
                return Ok(());
            };
            let read = spawn_blocking(move || recent(file.as_deref(), lines)).await?;
            let reply = match read {
                Ok(text) => Reply::Log { text },
                Err(e) => Reply::Failed {
                    reason: format!("{e:#}"),
                },
            };
            tx.send(&reply).await?;
            tx.finish().await
        })
    }
}
