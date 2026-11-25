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
fn projected_orders_reads_by_when_they_began() {
    let cache = Projected::<u32, String>::new(
        Duration::from_secs(10),
        Duration::from_millis(20),
        Duration::from_secs(2),
    );
    cache.put(1, "main".into());
    let before_write = cache.begin(&1);
    let other = cache.begin(&2);

    cache.put(1, "feature".into());
    cache.put(2, "elsewhere".into());

    assert!(!cache.put_begun(1, "main".into(), before_write));
    assert_eq!(cache.peek(&1), Some("feature".into()));
    assert!(cache.put_begun(3, "new".into(), cache.begin(&3)));
    assert!(!cache.put_begun(2, "stale".into(), other));

    // Of two overlapping reads the later one wins, whichever ends first; the earlier one
    // ending last is dropped.
    let first = cache.begin(&4);
    let second = cache.begin(&4);
    assert!(cache.put_begun(4, "second".into(), second));
    assert!(!cache.put_begun(4, "first".into(), first));
    assert_eq!(cache.peek(&4), Some("second".into()));

    let first = cache.begin(&5);
    let second = cache.begin(&5);
    assert!(cache.put_begun(5, "first".into(), first));
    assert!(cache.put_begun(5, "second".into(), second));
    assert_eq!(cache.peek(&5), Some("second".into()));
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

    let lane = JobKind::DeleteSession {
        agent: "claude".into(),
        session: session.clone(),
    }
    .lane();
    let (first_id, first_kind, _) = ops.next_due(machine, &lane).0.expect("relocate job");
    assert!(matches!(first_kind, JobKind::RelocateSession { folder, .. } if folder == "/work/b"));
    ops.settle_job(machine, first_id, Ok(()));

    let (second_id, second_kind, _) = ops.next_due(machine, &lane).0.expect("delete job");
    assert!(matches!(second_kind, JobKind::DeleteSession { .. }));
    ops.settle_job(machine, second_id, Ok(()));
    assert!(!ops.is_deleting_session_on(machine, &session));
    assert!(ops.next_due(machine, &lane).0.is_none());
}

/// A job of one lane waits for none of another, and an expired one is settled, not dropped.
#[test]
fn node_ops_runs_different_lanes_together_and_settles_a_job_that_expired_unsent() {
    let ops = NodeOps::default();
    let machine = sample_machine(9);
    let power = |on| JobKind::DevicePower {
        id: DeviceId("sim-1".into()),
        boot: on,
    };
    let stop = JobKind::StopProcess {
        session: SessionId::from("s-1"),
        pid: 7,
    };
    let (tx, rx) = oneshot::channel();

    assert!(ops.push_job(machine, power(true), None));
    assert!(ops.push_job(machine, stop.clone(), Some(tx)));
    let boot = ops
        .next_due(machine, &power(true).lane())
        .0
        .expect("the boot");
    assert!(matches!(boot.1, JobKind::DevicePower { .. }));

    // The boot is being sent; the stop is due all the same.
    let due = ops.next_due(machine, &stop.lane()).0.expect("the stop");
    assert!(matches!(due.1, JobKind::StopProcess { .. }));
    ops.settle_job(machine, due.0, Ok(()));
    assert_eq!(rx.blocking_recv().expect("heard"), Ok(()));
    assert!(ops.next_due(machine, &stop.lane()).0.is_none());

    // A job nobody sent in time is handed back, and its waiter hears of it.
    let (tx, mut rx) = oneshot::channel();
    assert!(ops.push_job(machine, stop.clone(), Some(tx)));
    {
        let mut held = ops.held.lock().unwrap();
        for job in held.queues.get_mut(&machine).unwrap() {
            job.deadline = Instant::now() - Duration::from_secs(1);
        }
    }
    let (due, expired) = ops.next_due(machine, &stop.lane());
    assert!(due.is_none());
    assert!(matches!(expired.as_slice(), [JobKind::StopProcess { .. }]));
    assert!(rx.try_recv().expect("heard").is_err());
}

fn found_gh() -> Value {
    use beep_proto::node::tool_update::{Available, Tool};

    Value::ToolUpdates(vec![Available {
        tool: Tool::Gh,
        version: "2.0.0".into(),
        latest: "2.1.0".into(),
    }])
}

#[test]
fn a_machine_that_pushes_its_tool_updates_is_not_asked() {
    let pushes = ToolPushes::default();
    let machine = sample_machine(1);

    assert_eq!(pushes.get(machine), None);
    assert!(pushes.heard(machine, Some(&found_gh())));

    let Some(ToolPush::Pushed(updates)) = pushes.get(machine) else {
        panic!("the list was not kept");
    };
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].tool, Tool::Gh);
    // The same list again changes nothing.
    assert!(!pushes.heard(machine, Some(&found_gh())));
    // Before its first check ends the node says nothing is found.
    assert!(pushes.heard(machine, Some(&Value::Missing)));
    assert_eq!(pushes.get(machine), Some(ToolPush::Pushed(Vec::new())));
}

#[test]
fn a_node_that_refuses_the_topic_keeps_the_check_call() {
    let pushes = ToolPushes::default();
    let machine = sample_machine(2);

    assert!(pushes.heard(machine, None));
    assert_eq!(pushes.get(machine), Some(ToolPush::Refused));
    assert!(!pushes.heard(machine, None));
    // A node updated since pushes from then on.
    assert!(pushes.heard(machine, Some(&found_gh())));
    assert!(matches!(pushes.get(machine), Some(ToolPush::Pushed(_))));
}

#[test]
fn an_installed_tool_leaves_the_pushed_list_until_the_node_pushes_again() {
    let pushes = ToolPushes::default();
    let machine = sample_machine(3);
    pushes.heard(machine, Some(&found_gh()));

    pushes.installed(machine, Tool::Gh);

    assert_eq!(pushes.get(machine), Some(ToolPush::Pushed(Vec::new())));
}
