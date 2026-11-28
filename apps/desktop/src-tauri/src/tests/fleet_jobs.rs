use super::*;
use iroh::SecretKey;

fn sample_machine(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
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

#[test]
fn a_repeated_delete_joins_the_one_being_sent() {
    let ops = NodeOps::default();
    let machine = sample_machine(11);
    let session = SessionId::from("s-2");
    let delete = || JobKind::DeleteSession {
        agent: "claude".into(),
        session: session.clone(),
    };
    let lane = delete().lane();

    assert!(ops.push_job(machine, delete(), None));
    let (id, _, _) = ops.next_due(machine, &lane).0.expect("delete job");
    assert!(!ops.push_job(machine, delete(), None));
    assert!(ops.is_deleting_session_on(machine, &session));

    ops.settle_job(machine, id, Ok(()));

    assert!(ops.next_due(machine, &lane).0.is_none());
    assert!(!ops.is_deleting_session_on(machine, &session));
}

#[test]
fn a_delete_after_a_move_is_queued_not_joined() {
    let ops = NodeOps::default();
    let machine = sample_machine(13);
    let session = SessionId::from("s-3");
    let delete = || JobKind::DeleteSession {
        agent: "claude".into(),
        session: session.clone(),
    };
    let lane = delete().lane();

    assert!(ops.push_job(machine, delete(), None));
    let (first, _, _) = ops.next_due(machine, &lane).0.expect("first delete");
    assert!(!ops.push_job(
        machine,
        JobKind::RelocateSession {
            agent: "claude".into(),
            session: session.clone(),
            folder: "/work/a".into(),
        },
        None,
    ));
    assert!(!ops.push_job(machine, delete(), None));

    ops.settle_job(machine, first, Ok(()));

    let (moved, kind, _) = ops.next_due(machine, &lane).0.expect("the move");
    assert!(matches!(kind, JobKind::RelocateSession { .. }));
    ops.settle_job(machine, moved, Ok(()));
    let (second, kind, _) = ops.next_due(machine, &lane).0.expect("the second delete");
    assert!(matches!(kind, JobKind::DeleteSession { .. }));
    ops.settle_job(machine, second, Ok(()));
    assert!(ops.next_due(machine, &lane).0.is_none());
}

/// A job whose send panics is dropped with an error for its waiter, and the next job of the lane
/// still runs on the same worker.
#[tokio::test]
async fn a_panicking_job_is_dropped_and_its_lane_goes_on() {
    use super::super::Fleet;
    use beep_mesh::transport_config;
    use beep_node::{
        agents::{Parts, detect},
        link::Offers,
        machine::provision::Provision,
    };
    use iroh::endpoint::presets;
    use std::sync::Arc;

    let dir = tempfile::TempDir::new().expect("a temp dir");
    let endpoint = iroh::Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport_config())
        .bind()
        .await
        .expect("the endpoint binds");
    let parts = Parts {
        kinds: detect(&[], Some(dir.path())),
        ..Parts::default()
    };
    let offers =
        Offers::local(parts, Provision::new(dir.path())).bound(&endpoint, dir.path(), "Studio");
    let fleet = Arc::new(Fleet::new(endpoint, dir.path(), "Studio", offers).await);
    let ops = Arc::new(NodeOps::default());
    let machine = sample_machine(14);
    let session = SessionId::from("s-4");
    let crash = JobKind::DeleteSession {
        agent: "panic".into(),
        session: session.clone(),
    };
    let lane = crash.lane();
    let (crashed_tx, crashed) = oneshot::channel();
    let (next_tx, next) = oneshot::channel();

    assert!(ops.push_job(machine, crash, Some(crashed_tx)));
    assert!(!ops.push_job(
        machine,
        JobKind::RelocateSession {
            agent: "ok".into(),
            session: session.clone(),
            folder: "/work/a".into(),
        },
        Some(next_tx),
    ));
    ops.run_lane(None, fleet, machine, lane).await;

    assert_eq!(crashed.await.expect("answered"), Err(JOB_FAILED.to_owned()));
    assert_eq!(next.await.expect("answered"), Ok(()));
    assert!(!ops.is_deleting_session_on(machine, &session));
}
