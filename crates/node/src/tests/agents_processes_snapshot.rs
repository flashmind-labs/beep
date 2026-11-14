use super::super::{UNWATCHED, WATCHED};
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
        reader.read(WATCHED, scan()),
        reader.read(WATCHED, scan()),
        reader.read(WATCHED, scan()),
    );
    let a = a.unwrap();
    assert!(Arc::ptr_eq(&a, &b.unwrap()));
    assert!(Arc::ptr_eq(&a, &c.unwrap()));
    assert_eq!(scans.load(Ordering::Relaxed), 1);

    tokio::time::advance(WATCHED).await;
    let next = reader.read(WATCHED, scan()).await.unwrap();
    assert!(!Arc::ptr_eq(&a, &next));
    assert_eq!(scans.load(Ordering::Relaxed), 2);
}

#[tokio::test(start_paused = true)]
async fn idle_sessions_share_a_table_up_to_their_interval_old() {
    let reader = Reader::default();
    let scans = AtomicUsize::new(0);
    let scan = || async {
        scans.fetch_add(1, Ordering::Relaxed);
        Ok(HashMap::new())
    };
    let first = reader.read(UNWATCHED, scan()).await.unwrap();

    // Idle sessions waking at their own times reuse it, while a watched
    // read wants a newer one.
    tokio::time::advance(UNWATCHED / 2).await;
    let idle = reader.read(UNWATCHED, scan()).await.unwrap();
    assert!(Arc::ptr_eq(&first, &idle));
    assert_eq!(scans.load(Ordering::Relaxed), 1);
    let watched = reader.read(WATCHED, scan()).await.unwrap();
    assert!(!Arc::ptr_eq(&first, &watched));
    assert_eq!(scans.load(Ordering::Relaxed), 2);

    // The newest table is what idle sessions share next.
    tokio::time::advance(UNWATCHED - WATCHED).await;
    let idle = reader.read(UNWATCHED, scan()).await.unwrap();
    assert!(Arc::ptr_eq(&watched, &idle));
    tokio::time::advance(WATCHED).await;
    reader.read(UNWATCHED, scan()).await.unwrap();
    assert_eq!(scans.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn stop_and_close_always_read_the_os_again() {
    let reader = Reader::default();
    let first = reader
        .read(WATCHED, async { Ok(HashMap::new()) })
        .await
        .unwrap();
    let fresh = reader
        .read(Duration::ZERO, async { Ok(HashMap::new()) })
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &fresh));
    let next = reader
        .read(WATCHED, async {
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
        .read(WATCHED, async { Ok(HashMap::new()) })
        .await
        .unwrap();
    tokio::time::advance(WATCHED).await;
    assert!(
        reader
            .read(WATCHED, async { anyhow::bail!("scan failed") })
            .await
            .is_err()
    );
    let scans = AtomicUsize::new(0);
    reader
        .read(WATCHED, async {
            scans.fetch_add(1, Ordering::Relaxed);
            Ok(HashMap::new())
        })
        .await
        .unwrap();
    assert_eq!(scans.load(Ordering::Relaxed), 1);
}
