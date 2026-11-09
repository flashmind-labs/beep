//! One OS process-table read shared by the sessions observing their children.

use super::{Process, os};
use crate::AsyncMutex;
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};
use tokio::time::Instant;

/// Matches the ancestry watch's cadence. Stop and close bypass it.
const FRESH: Duration = Duration::from_secs(1);

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
        fresh: bool,
        scan: impl Future<Output = anyhow::Result<HashMap<u32, Process>>>,
    ) -> anyhow::Result<Table> {
        // Coalesce concurrent reads instead of starting another full OS scan.
        let mut last = self.last.lock().await;

        if !fresh && let Some(snapshot) = last.as_ref().filter(|s| s.at.elapsed() < FRESH) {
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

pub(super) async fn read(fresh: bool) -> anyhow::Result<Table> {
    READER
        .get_or_init(Reader::default)
        .read(fresh, os::snapshot())
        .await
}

/// Signals changed the process table before its cached observation expires.
pub(super) async fn invalidate() {
    if let Some(reader) = READER.get() {
        reader.last.lock().await.take();
    }
}

#[cfg(test)]
#[path = "../../tests/agents_processes_snapshot.rs"]
mod tests;
