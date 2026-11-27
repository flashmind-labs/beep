//! One OS process-table read shared by the sessions observing their children.

use super::process::Process;
#[cfg(any(target_os = "linux", target_os = "android"))]
use super::linux as os;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use super::macos as os;
#[cfg(windows)]
use super::windows as os;
use crate::AsyncMutex;
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};
use tokio::time::Instant;

type Table = Arc<HashMap<u32, Process>>;

struct Snapshot {
    at: Instant,
    table: Table,
}

#[derive(Default)]
struct Reader {
    last: AsyncMutex<Option<Snapshot>>,
}

impl Reader {
    async fn read(
        &self,
        max_age: Duration,
        scan: impl Future<Output = anyhow::Result<HashMap<u32, Process>>>,
    ) -> anyhow::Result<Table> {
        // Coalesce concurrent reads instead of starting another full OS scan.
        let mut last = self.last.lock().await;

        if let Some(snapshot) = last.as_ref()
            && snapshot.at.elapsed() < max_age
        {
            return Ok(snapshot.table.clone());
        }

        let table = Arc::new(scan.await?);
        *last = Some(Snapshot {
            at: Instant::now(),
            table: table.clone(),
        });
        Ok(table)
    }
}

static READER: std::sync::OnceLock<Reader> = std::sync::OnceLock::new();

/// The table, read again unless the last read is younger than `max_age`;
/// `Duration::ZERO` always reads the OS.
pub(crate) async fn read(max_age: Duration) -> anyhow::Result<Table> {
    READER
        .get_or_init(Reader::default)
        .read(max_age, os::snapshot())
        .await
}

/// Signals changed the process table before its cached observation expires.
pub(crate) async fn invalidate() {
    if let Some(reader) = READER.get() {
        reader.last.lock().await.take();
    }
}

#[cfg(test)]
#[path = "../../tests/logs_processes_snapshot.rs"]
mod tests;
