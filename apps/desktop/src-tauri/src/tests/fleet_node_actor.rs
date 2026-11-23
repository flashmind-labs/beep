use super::*;
use iroh::SecretKey;
use std::sync::atomic::AtomicUsize;
use tokio::time::sleep;

fn sample_machine(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

#[tokio::test]
async fn projected_caps_cold_slow_read_once_and_notifies_when_ready() {
    let cache = Projected::<u32, String>::new(
        Duration::from_secs(10),
        Duration::from_millis(40),
        Duration::from_secs(2),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let notified = Arc::new(AtomicUsize::new(0));

    let started = Instant::now();
    let calls_a = calls.clone();
    let notified_a = notified.clone();
    let first = cache
        .read_or_default(
            1,
            || "pending".to_owned(),
            PartialEq::eq,
            move || {
                notified_a.fetch_add(1, Ordering::SeqCst);
            },
            move || {
                calls_a.fetch_add(1, Ordering::SeqCst);
                async move {
                    sleep(Duration::from_millis(120)).await;
                    Ok("ready".to_owned())
                }
            },
        )
        .await;
    assert_eq!(first, "pending");
    assert!(started.elapsed() < Duration::from_millis(100));

    // A second read while the first is still in flight returns immediately without waiting again.
    let second_start = Instant::now();
    let second = cache
        .read_or_default(
            1,
            || "pending".to_owned(),
            PartialEq::eq,
            || {},
            || async { Ok("unused".to_owned()) },
        )
        .await;
    assert_eq!(second, "pending");
    assert!(second_start.elapsed() < Duration::from_millis(15));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Once the slow node answers, `notify` fires and warm reads return immediately.
    sleep(Duration::from_millis(120)).await;
    assert_eq!(notified.load(Ordering::SeqCst), 1);
    let warm = cache
        .read_or_default(
            1,
            || "pending".to_owned(),
            PartialEq::eq,
            || {},
            || async { Ok("unused".to_owned()) },
        )
        .await;
    assert_eq!(warm, "ready");
}

#[tokio::test]
async fn projected_fast_cold_read_does_not_emit_redundant_notification() {
    let cache = Projected::<u32, String>::new(
        Duration::from_millis(30),
        Duration::from_millis(100),
        Duration::from_secs(2),
    );
    let notified = Arc::new(AtomicUsize::new(0));
    let n1 = notified.clone();
    let value = cache
        .read_or_default(
            7,
            String::new,
            PartialEq::eq,
            move || {
                n1.fetch_add(1, Ordering::SeqCst);
            },
            || async { Ok("v1".to_owned()) },
        )
        .await;
    assert_eq!(value, "v1");
    sleep(Duration::from_millis(40)).await;
    assert_eq!(notified.load(Ordering::SeqCst), 0);

    // Stale warm read returns v1 immediately and notifies once v2 arrives.
    let n2 = notified.clone();
    let stale_hit = cache
        .read_or_default(
            7,
            String::new,
            PartialEq::eq,
            move || {
                n2.fetch_add(1, Ordering::SeqCst);
            },
            || async { Ok("v2".to_owned()) },
        )
        .await;
    assert_eq!(stale_hit, "v1");
    sleep(Duration::from_millis(20)).await;
    assert_eq!(notified.load(Ordering::SeqCst), 1);
    assert_eq!(cache.peek(&7).as_deref(), Some("v2"));
}

#[tokio::test]
async fn projected_discards_in_flight_refresh_invalidated_by_mutation() {
    let cache = Projected::<u32, Vec<String>>::new(
        Duration::from_secs(10),
        Duration::from_millis(20),
        Duration::from_secs(2),
    );
    cache.put(1, vec!["s1".into(), "s2".into()]);

    // Force stale and start in-flight refreshes on keys 1 and 3.
    cache.forget(&1);
    let _ = cache
        .read_or_default(
            1,
            Vec::new,
            PartialEq::eq,
            || {},
            || async {
                sleep(Duration::from_millis(80)).await;
                Ok(vec!["s1".into(), "s2".into()])
            },
        )
        .await;
    let _ = cache
        .read_or_default(
            3,
            Vec::new,
            PartialEq::eq,
            || {},
            || async {
                sleep(Duration::from_millis(80)).await;
                Ok(vec!["remote-ok".into()])
            },
        )
        .await;

    // Mutating key 1 invalidates key 1, while mutating key 2 leaves key 3's slow refresh intact.
    cache.forget(&1);
    cache.put(2, vec!["local".into()]);
    sleep(Duration::from_millis(100)).await;

    assert_eq!(
        cache.peek(&1),
        None,
        "stale pre-mutation refresh must not resurrect forgotten entries"
    );
    assert_eq!(
        cache.peek(&3),
        Some(vec!["remote-ok".into()]),
        "mutating another key must not discard an unrelated machine's slow refresh"
    );
}

#[test]
fn node_ops_coalesces_unsent_jobs_and_tracks_optimistic_session_state() {
    let ops = NodeOps::default();
    let machine = sample_machine(9);
    let session = SessionId::from("s-1");

    assert!(ops.push_job(
        machine,
        JobKind::RelocateSession {
            agent: "claude".into(),
            session: session.clone(),
            folder: "/work/a".into(),
        },
        None,
    ));
    assert!(!ops.push_job(
        machine,
        JobKind::RelocateSession {
            agent: "claude".into(),
            session: session.clone(),
            folder: "/work/b".into(),
        },
        None,
    ));
    assert_eq!(
        ops.relocated_folder(&machine.to_string(), &session)
            .as_deref(),
        Some("/work/b")
    );

    // Deleting the session clears any pending relocation and marks it deleting.
    assert!(!ops.push_job(
        machine,
        JobKind::DeleteSession {
            agent: "claude".into(),
            session: session.clone(),
        },
        None,
    ));
    assert_eq!(ops.relocated_folder(&machine.to_string(), &session), None);
    assert!(ops.is_deleting_session_on(machine, &session));

    let (first_id, first_kind, _) = ops.next_due(machine).expect("relocate job");
    assert!(matches!(first_kind, JobKind::RelocateSession { folder, .. } if folder == "/work/b"));
    ops.settle_job(machine, first_id, Ok(()));

    let (second_id, second_kind, _) = ops.next_due(machine).expect("delete job");
    assert!(matches!(second_kind, JobKind::DeleteSession { .. }));
    ops.settle_job(machine, second_id, Ok(()));
    assert!(!ops.is_deleting_session_on(machine, &session));
    assert!(ops.next_due(machine).is_none());
}
