use super::*;
use beep_proto::node::{process, terminal};

fn demanded_machine(snapshots: &Snapshots, topic: &str) -> (EndpointId, Uuid) {
    let id = iroh::SecretKey::generate().public();
    snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::new(topic)])]));
    let generation = Uuid::new_v4();
    snapshots.begin(id, generation);
    assert!(snapshots.ready(id, generation));
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

#[test]
fn reconnect_clears_values_and_rejects_late_snapshots_from_the_previous_stream() {
    let snapshots = Snapshots::default();
    let (id, generation) = demanded_machine(&snapshots, "terminals");
    let current = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let query = snapshots.queries(id).pop().unwrap();
    let snapshot = terminal_snapshot(&query);
    assert_eq!(snapshots.install(id, generation, &snapshot), Some(true));
    assert!(current.borrow().is_some());
    let replacement = Uuid::new_v4();
    snapshots.begin(id, replacement);
    assert!(current.borrow().is_none());
    assert_eq!(snapshots.install(id, generation, &snapshot), None);
    assert!(current.borrow().is_none());
    snapshots.end(id, generation);
    assert!(snapshots.ready(id, replacement));
    assert_eq!(snapshots.install(id, replacement, &snapshot), Some(true));
    assert!(current.borrow().is_some());
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
    assert!(matches!(first.borrow().as_ref(), Some(Err(_))));
    let retry = snapshots.register(id, Read::TerminalList).unwrap().unwrap();
    let replacement = snapshots.queries(id).pop().unwrap();
    assert_ne!(query.id, replacement.id);
    assert!(retry.borrow().is_none());
    assert_eq!(
        snapshots.install(id, generation, &terminal_snapshot(&replacement)),
        Some(true)
    );
    assert!(matches!(retry.borrow().as_ref(), Some(Ok(_))));
}

#[test]
fn switching_an_exact_file_root_releases_its_previous_projection() {
    use beep_proto::node::agent::SessionsRequest;
    let snapshots = Snapshots::default();
    let id = iroh::SecretKey::generate().public();
    let root = "/srv/project-a";
    snapshots.reconcile(&BTreeMap::from([(id, vec![Topic::keyed("files", root)])]));
    let generation = Uuid::new_v4();
    snapshots.begin(id, generation);
    assert!(snapshots.ready(id, generation));
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
    snapshots.begin(id, generation);
    assert!(snapshots.ready(id, generation));
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
    snapshots.begin(id, generation);
    assert!(snapshots.register(id, Read::TerminalList).is_none());
    assert!(snapshots.queries(id).is_empty());
    assert!(!snapshots.ready(id, Uuid::new_v4()));
    assert!(snapshots.register(id, Read::TerminalList).is_none());
    assert!(snapshots.ready(id, generation));
    assert!(snapshots.register(id, Read::TerminalList).is_some());
    snapshots.end(id, generation);
    assert!(snapshots.register(id, Read::TerminalList).is_none());
}
