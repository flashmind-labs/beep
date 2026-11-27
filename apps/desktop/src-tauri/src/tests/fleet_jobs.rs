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
