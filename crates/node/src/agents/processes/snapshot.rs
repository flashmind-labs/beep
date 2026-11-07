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

pub(super) async fn read(fresh: bool) -> anyhow::Result<Table> {
    static READER: std::sync::OnceLock<Reader> = std::sync::OnceLock::new();
    READER
        .get_or_init(Reader::default)
        .read(fresh, os::snapshot())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(start_paused = true)]
    async fn concurrent_sessions_share_a_scan_until_it_expires() {
        let reader = Reader::default();
        let scans = AtomicUsize::new(0);
        let scan = || async {
            scans.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await;
            Ok(HashMap::new())
        };
        let (a, b, c) = tokio::join!(
            reader.read(false, scan()),
            reader.read(false, scan()),
            reader.read(false, scan()),
        );
        let a = a.unwrap();
        assert!(Arc::ptr_eq(&a, &b.unwrap()));
        assert!(Arc::ptr_eq(&a, &c.unwrap()));
        assert_eq!(scans.load(Ordering::Relaxed), 1);

        tokio::time::advance(FRESH).await;
        let next = reader.read(false, scan()).await.unwrap();
        assert!(!Arc::ptr_eq(&a, &next));
        assert_eq!(scans.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn stop_and_close_always_read_the_os_again() {
        let reader = Reader::default();
        let first = reader
            .read(false, async { Ok(HashMap::new()) })
            .await
            .unwrap();
        let fresh = reader
            .read(true, async { Ok(HashMap::new()) })
            .await
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &fresh));
        let next = reader
            .read(false, async {
                panic!("the fresh snapshot should be shared")
            })
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&fresh, &next));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_scan_is_retried_without_serving_an_expired_table() {
        let reader = Reader::default();
        reader
            .read(false, async { Ok(HashMap::new()) })
            .await
            .unwrap();
        tokio::time::advance(FRESH).await;
        assert!(
            reader
                .read(false, async { anyhow::bail!("scan failed") })
                .await
                .is_err()
        );
        let scans = AtomicUsize::new(0);
        reader
            .read(false, async {
                scans.fetch_add(1, Ordering::Relaxed);
                Ok(HashMap::new())
            })
            .await
            .unwrap();
        assert_eq!(scans.load(Ordering::Relaxed), 1);
    }
}
