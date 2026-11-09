//! `logs/1` (see `beep_proto::node::logs`): the node's recent log, asked by
//! the server.

use super::{recent, window};
use crate::link::{Protocol, Session};
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
pub struct Logs {
    source: Source,
}

#[derive(Debug, Clone, Default)]
enum Source {
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
        Self {
            source: if paths.is_empty() {
                Source::Service
            } else {
                Source::Files(paths)
            },
        }
    }
}

impl Logs {
    /// Discovers daily `beep.*.log` files on every query, including legacy logs.
    pub fn in_directory(dir: PathBuf) -> Self {
        Self {
            source: Source::Directory(dir),
        }
    }

    fn files(&self) -> Result<Vec<PathBuf>> {
        let dir = match &self.source {
            Source::Service => return Ok(Vec::new()),
            Source::Files(files) => return Ok(files.clone()),
            Source::Directory(dir) => dir,
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

    pub(crate) async fn serve_query(&self, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
        let Some(query) = rx.recv::<beep_proto::node::logs::Query>().await? else {
            return Ok(());
        };
        let logs = self.clone();
        let read = spawn_blocking(move || {
            let files = logs.files()?;

            if files.is_empty() && matches!(logs.source, Source::Directory(_)) {
                return Ok(String::new());
            }

            super::query(&files, &query)
        })
        .await?;
        let reply = match read {
            Ok(text) => Reply::Log { text },
            Err(e) => Reply::Failed {
                reason: format!("{e:#}"),
            },
        };
        tx.send(&reply).await?;
        tx.finish().await
    }

    /// The log from Unix second `since` on, the newest `max_bytes` of it.
    pub fn window(&self, since: i64, max_bytes: usize) -> Result<String> {
        let files = self.files()?;

        if files.is_empty() && matches!(self.source, Source::Directory(_)) {
            return Ok(String::new());
        }

        window(&files, since, max_bytes)
    }
}

impl Protocol for Logs {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        let logs = self.clone();
        Box::pin(async move {
            let Some(Request { lines }) = rx.recv().await? else {
                return Ok(());
            };
            let read = spawn_blocking(move || {
                let files = logs.files()?;

                if files.is_empty() && matches!(logs.source, Source::Directory(_)) {
                    return Ok(String::new());
                }

                recent(&files, lines)
            })
            .await?;
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
