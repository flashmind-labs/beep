use super::*;
use beep_proto::node::{process, terminal};
use beep_proto::watched;

fn demanded_machine(snapshots: &Snapshots, topic: &str) -> (EndpointId, Uuid) {
    let id = iroh::SecretKey::generate().public();
    snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::new(topic)])]));
    let generation = Uuid::new_v4();
    let topic = snapshots.held.lock().unwrap().machines[&id].topics[0].clone();
    snapshots.connected(id, topic, generation);

    (id, generation)
}

fn terminal_snapshot(query: &Query) -> Snapshot {
    Snapshot {
        topic: query.topic.clone(),
        query: Some(query.id),
        revision: 1,
        value: Value::Terminals(terminal::Reply::Terminals {
            terminals: Vec::new(),
        }),
    }
}

#[tokio::test]
async fn subscribed_reads_wait_for_the_first_snapshot_and_then_use_current_state() {
    let snapshots = Arc::new(Snapshots::default());
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let reader = snapshots.reader(id);
    let first = reader.snapshot(Read::TerminalList);
    tokio::pin!(first);
    assert!(futures::poll!(&mut first).is_pending());
    let query = snapshots.queries(id).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
    assert_eq!(first.await.unwrap().unwrap(), snapshot.value);
    assert_eq!(
        reader.snapshot(Read::TerminalList).await.unwrap().unwrap(),
        snapshot.value
    );
    assert_eq!(snapshots.queries(id), [query]);
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_that_comes_late_is_no_error_and_serves_the_next_read() {
    let snapshots = Arc::new(Snapshots::default());
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let reader = snapshots.reader(id);

    // The node is busy: the read gives up, so its caller reads the node directly.
    assert!(reader.snapshot(Read::TerminalList).await.is_none());

    let query = snapshots.queries(id).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));

    assert_eq!(
        reader.snapshot(Read::TerminalList).await.unwrap().unwrap(),
        snapshot.value
    );
}

#[test]
fn reconnect_clears_values_and_rejects_late_snapshots_from_the_previous_stream() {
    let snapshots = Snapshots::default();
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let query = snapshots.queries(id).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
    assert!(watched::read(&current, Option::is_some));
    let replacement = Uuid::new_v4();
    snapshots.connected(id, Topic::new("terminals"), replacement);
    assert!(watched::read(&current, Option::is_none));
    assert_eq!(snapshots.install(id, generation, &snapshot), None);
    assert!(watched::read(&current, Option::is_none));
    snapshots.end(id, generation);

    assert_eq!(snapshots.install(id, replacement, &snapshot), Some(true));
    assert!(watched::read(&current, Option::is_some));
}

#[test]
fn removing_demand_drops_queries_and_wakes_pending_readers() {
    let snapshots = Snapshots::default();
    let (id, _) = demanded_machine(&snapshots, "terminals");
    let current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    snapshots.reconcile(&BTreeMap::new());
    assert!(current.has_changed().is_err());
    assert!(snapshots.queries(id).is_empty());
    assert!(snapshots.register(id, Read::TerminalList).is_none());
    assert!(
        snapshots
            .register(
                id,
                Read::Process(process::Request::Start {
                    command: "true".into(),
                    cwd: None
                })
            )
            .is_none()
    );
}

#[test]
fn query_storage_evicts_the_least_recent_read_and_keeps_reused_projections() {
    let snapshots = Snapshots::default();
    let (id, _) = demanded_machine(&snapshots, "process-output");
    let first = Read::Process(process::Request::Status {
        id: process::TaskId(0),
    });

    for number in 0..u32::try_from(events::MAX_QUERIES).unwrap() {
        snapshots
            .register(
                id,
                Read::Process(process::Request::Status {
                    id: process::TaskId(number),
                }),
            )
            .unwrap()
            .unwrap();
    }

    snapshots.register(id, first.clone()).unwrap().unwrap();
    snapshots
        .register(
            id,
            Read::Process(process::Request::Status {
                id: process::TaskId(u32::MAX),
            }),
        )
        .unwrap()
        .unwrap();
    let queries = snapshots.queries(id);
    assert_eq!(queries.len(), events::MAX_QUERIES);
    assert!(queries.iter().any(|query| query.read == first));
    assert!(!queries.iter().any(|query| query.read
        == Read::Process(process::Request::Status {
            id: process::TaskId(1)
        })));
}

#[test]
fn a_failed_read_registers_a_new_projection_on_the_next_attempt() {
    let snapshots = Snapshots::default();
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let first = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let query = snapshots.queries(id).pop().unwrap();
    snapshots.failed(id, generation, query.id, "temporary read failure".into());
    assert!(watched::read(&first, |value| matches!(value, Some(Err(_)))));
    let retry = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let replacement = snapshots.queries(id).pop().unwrap();
    assert_ne!(query.id, replacement.id);
    assert!(watched::read(&retry, Option::is_none));
    assert_eq!(
        snapshots.install(id, generation, &terminal_snapshot(&replacement)),
        Some(true)
    );
    assert!(watched::read(&retry, |value| matches!(value, Some(Ok(_)))));
}

#[test]
fn switching_an_exact_file_root_releases_its_previous_projection() {
    use beep_proto::node::agent::SessionsRequest;
    let snapshots = Snapshots::default();
    let id = iroh::SecretKey::generate().public();
    let root = "/srv/project-a";
    snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::keyed("files", root)])]));
    let generation = Uuid::new_v4();
    let topic = snapshots.held.lock().unwrap().machines[&id].topics[0].clone();
    snapshots.connected(id, topic, generation);

    let read = Read::Sessions(Box::new(SessionsRequest::Folder {
        root: root.into(),
        path: String::new(),
        ignored: false,
        depth: 1,
    }));
    let previous = snapshots.register(id, read.clone()).unwrap().unwrap();
    snapshots.reconcile(&BTreeMap::from([(
        id,
        vec![Topic::keyed("files", "/srv/project-b")],
    )]));
    assert!(previous.has_changed().is_err());
    assert!(snapshots.queries(id).is_empty());
    assert!(snapshots.register(id, read).is_none());
}

#[test]
fn switching_a_project_alias_releases_its_physical_file_projection() {
    use beep_proto::node::agent::SessionsRequest;
    let snapshots = Snapshots::default();
    let id = iroh::SecretKey::generate().public();
    snapshots.reconcile(&BTreeMap::from([(
        id,
        vec![Topic::parse("files.org.project-a").unwrap()],
    )]));
    let generation = Uuid::new_v4();
    let topic = snapshots.held.lock().unwrap().machines[&id].topics[0].clone();
    snapshots.connected(id, topic, generation);

    let read = Read::Sessions(Box::new(SessionsRequest::Folder {
        root: "/srv/project-a".into(),
        path: String::new(),
        ignored: false,
        depth: 1,
    }));
    let previous = snapshots.register(id, read).unwrap().unwrap();
    snapshots.reconcile(&BTreeMap::from([(
        id,
        vec![Topic::parse("files.org.project-b").unwrap()],
    )]));
    assert!(previous.has_changed().is_err());
    assert!(snapshots.queries(id).is_empty());
}

#[test]
fn the_first_query_value_is_distinct_from_live_updates_and_error_recovery() {
    let snapshots = Snapshots::default();
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let _current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let query = snapshots.queries(id).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(false));
    assert!(snapshots.failed(id, generation, query.id, "temporary failure".into()));
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(false));
    let topic = Snapshot {
        query: None,
        ..snapshot
    };
    assert_eq!(snapshots.install(id, generation, &topic), Some(false));
}

#[test]
fn reads_use_finite_rpc_until_a_stream_is_accepted_and_after_it_ends() {
    let snapshots = Snapshots::default();
    let id = iroh::SecretKey::generate().public();
    assert!(snapshots.register(id, Read::TerminalList).is_none());
    snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::new("terminals")])]));
    assert!(snapshots.register(id, Read::TerminalList).is_none());
    let generation = Uuid::new_v4();
    let topic = snapshots.held.lock().unwrap().machines[&id].topics[0].clone();
    assert!(snapshots.queries(id).is_empty());
    assert!(snapshots.connected(id, topic, generation));
    assert!(snapshots.register(id, Read::TerminalList).is_some());
    snapshots.end(id, generation);
    assert!(snapshots.register(id, Read::TerminalList).is_none());
}

#[test]
fn reconnecting_one_subscription_preserves_other_subscriptions_and_their_queries() {
    let snapshots = Snapshots::default();
    let id = iroh::SecretKey::generate().public();
    let terminals = Topic::new("terminals");
    let processes = Topic::new("processes");
    snapshots.reconcile(&BTreeMap::from([(
        id,
        vec![terminals.clone(), processes.clone()],
    )]));
    let terminal_generation = Uuid::new_v4();
    let process_generation = Uuid::new_v4();
    snapshots.connected(id, terminals.clone(), terminal_generation);
    snapshots.connected(id, processes.clone(), process_generation);

    let terminal_value = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let process_value = snapshots
        .register(id, Read::Process(process::Request::List {}))
        .unwrap()
        .unwrap();
    let query = snapshots.queries_for(id, &terminals.clone()).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    assert_eq!(
        snapshots.install(id, terminal_generation, &snapshot),
        Some(true)
    );
    assert_eq!(snapshots.install(id, process_generation, &snapshot), None);
    assert!(!snapshots.failed(id, process_generation, query.id, "wrong stream".into()));

    let replacement = Uuid::new_v4();
    snapshots.connected(id, processes.clone(), replacement);
    snapshots.end(id, process_generation);

    assert_eq!(
        watched::read(&terminal_value, |value| value.clone()),
        Some(Ok(snapshot.value))
    );
    assert!(watched::read(&process_value, Option::is_none));
    assert_eq!(snapshots.queries_for(id, &terminals.clone()), [query]);

    snapshots.end(id, replacement);
    assert!(process_value.has_changed().is_err());
    assert!(!terminal_value.has_changed().is_err());
    assert!(snapshots.queries_for(id, &processes.clone()).is_empty());
}

#[test]
fn removing_one_subscription_keeps_other_stream_generations_and_cached_values() {
    let snapshots = Snapshots::default();
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let query = snapshots.queries(id).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    snapshots.install(id, generation, &snapshot);
    snapshots.reconcile(&BTreeMap::from([(
        id,
        vec![Topic::new("terminals"), Topic::new("processes")],
    )]));
    let other = Uuid::new_v4();
    snapshots.connected(id, Topic::new("processes"), other);

    snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::new("terminals")])]));
    snapshots.end(id, other);
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(false));
    assert!(watched::read(&current, Option::is_some));
    assert!(!snapshots.connected(id, Topic::new("processes"), other));
}

#[test]
fn a_changed_query_nobody_reads_again_is_dropped_after_the_linger() {
    let snapshots = Snapshots::default();
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let topic = Topic::new("terminals");
    drop(snapshots.register(id, Read::TerminalList).unwrap().unwrap());
    let query = snapshots.queries(id).pop().unwrap();
    let first = terminal_snapshot(&query);
    let start = Instant::now();
    assert_eq!(snapshots.install(id, generation, &first), Some(true));
    // The first value is what the read waited for; it does not count as unread.
    assert!(!snapshots.sweep(id, &topic, start + LINGER * 2));

    let changed = Snapshot {
        revision: 2,
        ..first.clone()
    };
    assert_eq!(snapshots.install(id, generation, &changed), Some(false));
    let changed_at = Instant::now();
    // A read again within the linger keeps it.
    drop(snapshots.register(id, Read::TerminalList).unwrap().unwrap());
    assert!(!snapshots.sweep(id, &topic, changed_at + LINGER * 2));
    assert_eq!(snapshots.queries(id), std::slice::from_ref(&query));

    // Changed again and never read: gone after the linger, and the
    // subscription is told to send the node its smaller set.
    assert_eq!(snapshots.install(id, generation, &changed), Some(false));
    let unread_at = Instant::now();
    let mut told = snapshots.changes();
    told.mark_unchanged();
    assert!(!snapshots.sweep(id, &topic, unread_at + LINGER / 2));
    assert_eq!(snapshots.queries(id).len(), 1);
    assert!(snapshots.sweep(id, &topic, unread_at + LINGER));
    assert!(snapshots.queries(id).is_empty());
    assert!(told.has_changed().unwrap());
}

#[test]
fn a_waiting_reader_keeps_its_query() {
    let snapshots = Snapshots::default();
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let waiting = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let query = snapshots.queries(id).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    snapshots.install(id, generation, &snapshot);
    snapshots.install(id, generation, &snapshot);
    assert!(!snapshots.sweep(id, &Topic::new("terminals"), Instant::now() + LINGER * 2));
    drop(waiting);
}

#[tokio::test]
async fn a_wait_keeps_its_topic_subscribed_while_no_view_wants_it() {
    let snapshots = Arc::new(Snapshots::default());
    let id = iroh::SecretKey::generate().public();
    let reader = snapshots.reader(id);
    let hold = reader.hold(&Read::SyncList);

    // What the events loop does with no view open.
    let mut wanted = BTreeMap::new();
    snapshots.add_holds(&mut wanted, &[id]);
    snapshots.reconcile(&wanted);
    let topics = wanted.remove(&id).unwrap();
    assert_eq!(topics, [Read::SyncList.topic().unwrap()]);

    let follow = reader.follow(Read::SyncList);
    tokio::pin!(follow);
    assert!(futures::poll!(&mut follow).is_pending());

    let generation = Uuid::new_v4();
    assert!(snapshots.connected(id, topics[0].clone(), generation));
    let current = follow.await.unwrap();
    let query = snapshots.queries(id).pop().unwrap();
    let status = Value::Sync(Box::new(beep_proto::node::sync::Reply::Status {
        projects: Vec::new(),
    }));
    let snapshot = Snapshot {
        topic: query.topic.clone(),
        query: Some(query.id),
        revision: 1,
        value: status.clone(),
    };
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
    assert_eq!(
        watched::read(&current, |held| held.clone()),
        Some(Ok(status))
    );

    drop(hold);
    let mut wanted = BTreeMap::new();
    snapshots.add_holds(&mut wanted, &[id]);
    assert!(wanted.is_empty());
}
