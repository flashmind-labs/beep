//! `logs/1` (see `beep_proto::node::logs`): the node's recent log, asked by
//! the server.

use super::{recent, window};
use crate::link::{Protocol, Session};
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    logs::{PROTOCOL, Reply, Request},
};
use std::path::PathBuf;
use tokio::task::spawn_blocking;

/// The node's log: the service's, or log files of the host's. Serves
/// `logs/1`.
#[derive(Debug, Clone, Default)]
pub struct Logs {
    /// Oldest first; none is the service's log.
    files: Vec<PathBuf>,
}

impl Logs {
    /// The log `beep-node logs` shows.
    pub fn service() -> Self {
        Self::default()
    }

    /// The log file at `path`, for a host that keeps its own, as the Mac app.
    pub fn file(path: PathBuf) -> Self {
        Self::files(vec![path])
    }

    /// The log files at `paths`, oldest first, the last one being written.
    pub fn files(paths: Vec<PathBuf>) -> Self {
        Self { files: paths }
    }

    /// The log from Unix second `since` on, the newest `max_bytes` of it.
    pub fn window(&self, since: i64, max_bytes: usize) -> Result<String> {
        window(&self.files, since, max_bytes)
    }
}

impl Protocol for Logs {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        let file = self.files.last().cloned();
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
