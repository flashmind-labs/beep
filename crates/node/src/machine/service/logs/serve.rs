//! `logs/1` (see `beep_proto::node::logs`): the node's recent log, asked by
//! the server.

use super::{recent, window};
use crate::protocol::{Protocol, Session};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    logs::{PROTOCOL, Reply, Request},
};
use std::{fs, io::ErrorKind, path::PathBuf};
use tokio::task::spawn_blocking;

/// The node's log: the service's, or log files of the host's. Serves
/// `logs/1`.
#[derive(Debug, Clone, Default)]
pub enum Logs {
    #[default]
    Service,
    Files(Vec<PathBuf>),
    Directory(PathBuf),
}

impl From<PathBuf> for Logs {
    /// The log file at `path`, for a host that keeps its own, as the Mac app.
    fn from(path: PathBuf) -> Self {
        Self::from(vec![path])
    }
}

impl From<Vec<PathBuf>> for Logs {
    /// The log files at `paths`, oldest first, the last one being written.
    fn from(paths: Vec<PathBuf>) -> Self {
        if paths.is_empty() {
            Self::Service
        } else {
            Self::Files(paths)
        }
    }
}

impl Logs {
    /// Discovers daily `beep.*.log` files on every query, including legacy logs.
    pub fn in_directory(dir: PathBuf) -> Self {
        Self::Directory(dir)
    }

    fn files(&self) -> Result<Vec<PathBuf>> {
        let dir = match self {
            Self::Service => return Ok(Vec::new()),
            Self::Files(files) => return Ok(files.clone()),
            Self::Directory(dir) => dir,
        };
        let mut files = Vec::new();

        for entry in fs::read_dir(dir).context("reading the log directory")? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if !name.starts_with("beep.") || !name.ends_with(".log") {
                continue;
            }

            let metadata = match entry.metadata() {
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                metadata => metadata?,
            };

            if metadata.is_file() {
                files.push((metadata.modified()?, entry.path()));
            }
        }

        files.sort();
        Ok(files.into_iter().map(|(_, path)| path).collect())
    }

    /// `read` of the log files, with nothing to read in an empty directory.
    fn read(&self, read: impl FnOnce(&[PathBuf]) -> Result<String>) -> Result<String> {
        let files = self.files()?;

        if files.is_empty() && matches!(self, Self::Directory(_)) {
            return Ok(String::new());
        }

        read(&files)
    }

    /// Reads on a blocking thread and answers the caller with the text or
    /// why it could not be read.
    async fn answer(
        &self,
        mut tx: LinkTx,
        read: impl FnOnce(&[PathBuf]) -> Result<String> + Send + 'static,
    ) -> Result<()> {
        let logs = self.clone();
        let read = spawn_blocking(move || logs.read(read)).await?;
        let reply = match read {
            Ok(text) => Reply::Log { text },
            Err(e) => Reply::Failed {
                reason: format!("{e:#}"),
            },
        };
        tx.send(&reply).await?;
        tx.finish().await
    }

    pub(crate) async fn serve_query(&self, tx: LinkTx, mut rx: LinkRx) -> Result<()> {
        let Some(query) = rx.recv::<beep_proto::node::logs::Query>().await? else {
            return Ok(());
        };

        self.answer(tx, move |files| super::query(files, &query))
            .await
    }

    /// The log from Unix second `since` on, the newest `max_bytes` of it.
    pub fn window(&self, since: i64, max_bytes: usize) -> Result<String> {
        self.read(|files| window(files, since, max_bytes))
    }
}

impl Protocol for Logs {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, tx: LinkTx, mut rx: LinkRx) -> Session {
        let logs = self.clone();
        Box::pin(async move {
            let Some(Request { lines }) = rx.recv().await? else {
                return Ok(());
            };

            logs.answer(tx, move |files| recent(files, lines)).await
        })
    }
}
