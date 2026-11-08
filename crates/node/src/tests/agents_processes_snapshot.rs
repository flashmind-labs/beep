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
