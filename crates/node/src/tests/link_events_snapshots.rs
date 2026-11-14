use super::*;

async fn stepped_snapshot(snapshots: &mut Snapshots) -> Option<Reply> {
    let next = snapshots.next();
    tokio::pin!(next);

    loop {
        if let std::task::Poll::Ready(result) = futures::poll!(&mut next) {
            return result.unwrap();
        }
        // Keep paused time fixed while the blocking encoder completes.
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn session_process_topics_read_children_and_follow_process_changes() {
    let offers = Offers::default();
    let caller = iroh::SecretKey::generate().public();
    let topic = Topic::keyed("processes", "example-session");
    let mut snapshots = Snapshots::new();
    snapshots.replace(&offers, std::slice::from_ref(&topic), &[], &BTreeMap::new());
    let key = (topic.clone(), None);
    assert_eq!(
        snapshots.entries.get(&key).map(|entry| &entry.source),
        Some(&Source::Read(Read::Sessions(Box::new(
            SessionsRequest::Processes {
                session: "example-session".into(),
            }
        ))))
    );
    snapshots.start(&offers, caller);
    let Some(Reply::Snapshot(initial)) = stepped_snapshot(&mut snapshots).await else {
        panic!("expected the session's initial process list");
    };
    assert_eq!(initial.topic, topic);
    assert_eq!(
        initial.value,
        Value::Sessions(Box::new(SessionsReply::Processes { processes: vec![] }))
    );

    snapshots.changed(&Topic::new("processes"), ChangeKind::Any);
    assert!(snapshots.entries.get(&key).unwrap().dirty);
    // Completing the session read with a new child must produce another
    // snapshot even though the separate managed-command table did not change.
    let children = Value::Sessions(Box::new(SessionsReply::Processes {
        processes: vec![beep_proto::node::agent::RunningProcess {
            pid: 123,
            command: "sleep 300".into(),
            started_at: 1,
            ports: vec![],
            left_group: false,
            parent: 0,
        }],
    }));
    let result = children.clone();
    let generation = snapshots.generation;
    snapshots.reads.spawn(async move {
        Completed {
            generation,
            key,
            result: Ok((result, 128)),
        }
    });
    let Some(Reply::Snapshot(changed)) = stepped_snapshot(&mut snapshots).await else {
        panic!("expected the child-process update");
    };
    assert_eq!(changed.topic, topic);
    assert_eq!(changed.value, children);
    assert!(changed.revision > initial.revision);
}

#[tokio::test(start_paused = true)]
async fn plugin_bursts_coalesce_without_idle_timers_or_losing_inflight_changes() {
    let offers = Offers::default();
    let caller = iroh::SecretKey::generate().public();
    let topic = Topic::keyed("plugin", "example");
    let mut snapshots = Snapshots::new();
    offers
        .agents
        .events
        .publish(topic.clone(), vec![1])
        .unwrap();
    snapshots.replace(&offers, std::slice::from_ref(&topic), &[], &BTreeMap::new());
    let started = Instant::now();
    assert_eq!(snapshots.due(), Some(started));
    snapshots.start(&offers, caller);
    assert!(snapshots.due().is_none());
    assert!(
        matches!(stepped_snapshot(&mut snapshots).await, Some(Reply::Snapshot(value)) if value.value == Value::Plugin(vec![1]))
    );
    assert_eq!(Instant::now(), started);
    assert!(snapshots.due().is_none());

    offers
        .agents
        .events
        .publish(topic.clone(), vec![2])
        .unwrap();
    snapshots.changed(&topic, ChangeKind::Any);
    snapshots.start(&offers, caller);
    offers
        .agents
        .events
        .publish(topic.clone(), vec![3])
        .unwrap();
    snapshots.changed(&topic, ChangeKind::Any);
    assert!(snapshots.reads.is_empty());
    assert_eq!(snapshots.due(), Some(started + READ_GAP));
    tokio::time::advance(READ_GAP - Duration::from_millis(1)).await;
    snapshots.start(&offers, caller);
    assert!(snapshots.reads.is_empty());
    tokio::time::advance(Duration::from_millis(1)).await;
    snapshots.start(&offers, caller);
    // A notification arriving during a read must retain another dirty pass.
    snapshots.changed(&topic, ChangeKind::Any);
    assert!(
        matches!(stepped_snapshot(&mut snapshots).await, Some(Reply::Snapshot(value)) if value.value == Value::Plugin(vec![3]))
    );
    assert_eq!(snapshots.due(), Some(started + READ_GAP + READ_GAP));
    offers.agents.events.publish(topic, vec![4]).unwrap();
    snapshots.start(&offers, caller);
    assert!(snapshots.reads.is_empty());
    tokio::time::advance(READ_GAP).await;
    snapshots.start(&offers, caller);
    assert!(
        matches!(stepped_snapshot(&mut snapshots).await, Some(Reply::Snapshot(value)) if value.value == Value::Plugin(vec![4]))
    );
    assert!(snapshots.due().is_none());
}

#[tokio::test]
async fn recovery_after_a_read_error_resends_identical_state() {
    let mut snapshots = Snapshots::new();
    let topic = Topic::keyed("plugin", "example");
    let key = (topic.clone(), Some(7));
    let value = Value::Plugin(vec![1]);
    snapshots.entries.insert(
        key.clone(),
        Projection {
            topic,
            query: Some(7),
            source: Source::Plugin,
            previous: Some(value.clone()),
            bytes: 1,
            dirty: false,
            running: true,
            not_before: Instant::now(),
            idle: 0,
        },
    );
    let failed = key.clone();
    snapshots.reads.spawn(async move {
        Completed {
            generation: 0,
            key: failed,
            result: Err(anyhow::anyhow!("temporary read failure")),
        }
    });
    assert!(matches!(
        snapshots.next().await.unwrap(),
        Some(Reply::SnapshotError { query: Some(7), .. })
    ));
    let entry = snapshots.entries.get(&key).unwrap();
    assert!(entry.previous.is_none());
    assert_eq!(entry.bytes, 0);
    assert!(!entry.running);
    let restored = value.clone();
    snapshots.reads.spawn(async move {
        Completed {
            generation: 0,
            key,
            result: Ok((restored, 1)),
        }
    });
    let Some(Reply::Snapshot(snapshot)) = snapshots.next().await.unwrap() else {
        panic!("expected recovered state")
    };
    assert_eq!(snapshot.value, value);
    assert_eq!(snapshot.query, Some(7));
    assert_eq!(snapshot.revision, 1);
}

#[tokio::test(start_paused = true)]
async fn a_burst_of_changes_reads_once_more_and_delivers_the_last_state() {
    let offers = Offers::default();
    let caller = iroh::SecretKey::generate().public();
    let topic = Topic::keyed("plugin", "burst");
    let mut snapshots = Snapshots::new();
    offers
        .agents
        .events
        .publish(topic.clone(), vec![0])
        .unwrap();
    snapshots.replace(&offers, std::slice::from_ref(&topic), &[], &BTreeMap::new());
    snapshots.start(&offers, caller);
    assert!(matches!(
        stepped_snapshot(&mut snapshots).await,
        Some(Reply::Snapshot(value)) if value.value == Value::Plugin(vec![0])
    ));
    let mut reads = 0;

    // A hundred changes inside one gap start no read until the gap ends.
    for step in 1..=100u8 {
        offers
            .agents
            .events
            .publish(topic.clone(), vec![step])
            .unwrap();
        snapshots.changed(&topic, ChangeKind::Any);
        snapshots.start(&offers, caller);
        reads += snapshots.reads.len();
        tokio::time::advance(READ_GAP / 200).await;
    }

    assert_eq!(reads, 0);
    tokio::time::advance(READ_GAP).await;
    snapshots.start(&offers, caller);
    assert_eq!(snapshots.reads.len(), 1);
    assert!(matches!(
        stepped_snapshot(&mut snapshots).await,
        Some(Reply::Snapshot(value)) if value.value == Value::Plugin(vec![100])
    ));
    assert!(snapshots.due().is_none());
}

#[tokio::test(start_paused = true)]
async fn one_session_topic_reads_that_session_on_a_shorter_gap() {
    let offers = Offers::default();
    let caller = iroh::SecretKey::generate().public();
    let topic = Topic::keyed("agent", "example-session");
    let mut snapshots = Snapshots::new();
    snapshots.replace(&offers, std::slice::from_ref(&topic), &[], &BTreeMap::new());
    let key = (topic.clone(), None);
    assert_eq!(
        snapshots.entries.get(&key).map(|entry| &entry.source),
        Some(&Source::Agent("example-session".into()))
    );
    let started = Instant::now();
    snapshots.start(&offers, caller);
    let Some(Reply::Snapshot(initial)) = stepped_snapshot(&mut snapshots).await else {
        panic!("expected the session's snapshot");
    };
    assert_eq!(
        initial.value,
        Value::Sessions(Box::new(SessionsReply::Sessions { sessions: vec![] }))
    );

    snapshots.changed(&topic, ChangeKind::Any);
    assert!(AGENT_GAP < READ_GAP);
    assert_eq!(snapshots.due(), Some(started + AGENT_GAP));
}

#[test]
fn work_tree_changes_skip_git_projections_but_refresh_folders() {
    let offers = Offers::default();
    let root = "/srv/project";
    let topic = Topic::keyed("files", root);
    let git = Query {
        id: 1,
        topic: topic.clone(),
        read: Read::Git {
            root: root.into(),
            query: beep_proto::node::events::GitRead::State,
        },
    };
    let folder = Query {
        id: 2,
        topic: topic.clone(),
        read: Read::Sessions(Box::new(SessionsRequest::Folder {
            root: root.into(),
            path: String::new(),
            ignored: false,
            depth: 1,
        })),
    };
    let mut snapshots = Snapshots::new();
    snapshots.replace(&offers, &[], &[git, folder], &BTreeMap::new());
    let dirty = |snapshots: &Snapshots, id| snapshots.entries[&(topic.clone(), Some(id))].dirty;

    for entry in snapshots.entries.values_mut() {
        entry.dirty = false;
    }
    snapshots.changed(&topic, ChangeKind::WorkTree);
    assert!(!dirty(&snapshots, 1));
    assert!(dirty(&snapshots, 2));

    snapshots.changed(&topic, ChangeKind::Any);
    assert!(dirty(&snapshots, 1));
}

#[test]
fn outside_session_lists_wait_longer_between_reads() {
    let external = Source::Read(Read::Sessions(Box::new(SessionsRequest::External {
        agent: "claude".into(),
        limit: 20,
        session: None,
    })));
    let folders = Source::Read(Read::Sessions(Box::new(SessionsRequest::InFolders {
        folders: vec!["/srv/project".into()],
        limit: 20,
    })));
    let beep = Source::Read(Read::Sessions(Box::new(SessionsRequest::ListAll {})));
    assert_eq!(external.gap(), SCAN_GAP);
    assert_eq!(folders.gap(), SCAN_GAP);
    assert_eq!(beep.gap(), READ_GAP);
}

/// A desktop up to 0.8.3 kept the found checkouts live, and the node looked
/// at every checkout again after each session write.
#[test]
fn found_checkouts_kept_live_wait_a_minute_between_reads() {
    let found = Source::Read(Read::Sessions(Box::new(SessionsRequest::Folders {
        limit: 100,
    })));
    assert_eq!(found.gap(), FOLDERS_GAP);
}

/// Desktops up to 0.8.4 kept a live lookup of each session row's outside
/// session; every line any agent wrote made the node look again.
#[tokio::test(start_paused = true)]
async fn a_kept_lookup_of_one_outside_session_is_not_read_again_on_every_change() {
    let offers = Offers::default();
    let caller = iroh::SecretKey::generate().public();
    let read = Read::Sessions(Box::new(SessionsRequest::External {
        agent: "codex".into(),
        limit: 1,
        session: Some("example-session".into()),
    }));
    let query = Query {
        id: 1,
        topic: read.topic().expect("a topic"),
        read,
    };
    let mut snapshots = Snapshots::new();
    snapshots.replace(
        &offers,
        &[Topic::new("agents")],
        std::slice::from_ref(&query),
        &BTreeMap::new(),
    );
    let key = (query.topic.clone(), Some(1));
    let started = Instant::now();
    snapshots.start(&offers, caller);
    assert!(snapshots.entries[&key].running);
    while snapshots.entries[&key].running {
        let _ = stepped_snapshot(&mut snapshots).await;
    }

    // An agent writes a line every 100 ms for ten seconds.
    let mut reads = 0;
    for _ in 0..100 {
        tokio::time::advance(Duration::from_millis(100)).await;
        snapshots.changed(&Topic::keyed("agent", "busy"), ChangeKind::Any);
        snapshots.start(&offers, caller);

        if snapshots.entries[&key].running {
            reads += 1;
            while snapshots.entries[&key].running {
                let _ = stepped_snapshot(&mut snapshots).await;
            }
        }
    }

    assert_eq!(reads, 0, "read again {reads} times");
    assert_eq!(snapshots.due(), Some(started + SESSION_GAP));
}

/// A read of a folder that keeps coming back the same, a desktop's or a
/// `files` topic's own, waits longer each time, up to a bound, and reads
/// promptly again once it changes.
#[test]
fn an_unchanged_file_read_backs_off() {
    let source = Source::Read(Read::Sessions(Box::new(SessionsRequest::ReadFile {
        root: "/srv/project".into(),
        path: "a.txt".into(),
        offset: 0,
        len: 10,
    })));
    let projection = |query: Option<u64>, idle: u32| Projection {
        topic: Topic::keyed("files", "/srv/project"),
        query,
        source: source.clone(),
        previous: None,
        bytes: 0,
        dirty: false,
        running: false,
        not_before: Instant::now(),
        idle,
    };
    assert_eq!(projection(Some(1), 0).gap(), READ_GAP);
    assert_eq!(projection(Some(1), 1).gap(), READ_GAP * 2);
    assert_eq!(projection(Some(1), 50).gap(), READ_GAP * 8);
    assert_eq!(projection(None, 50).gap(), READ_GAP * 8);
    let list = Projection {
        source: Source::Read(Read::Sessions(Box::new(SessionsRequest::ListAll {}))),
        ..projection(Some(1), 50)
    };
    assert_eq!(list.gap(), READ_GAP);
}

/// A Git read whose command keeps failing ran git again on every change, four
/// times a second; it backs off like an unchanged read and recovers at once.
#[tokio::test(start_paused = true)]
async fn a_failing_git_read_backs_off_until_it_succeeds() {
    let offers = Offers::default();
    let caller = iroh::SecretKey::generate().public();
    let root = "/srv/missing-project";
    let query = Query {
        id: 1,
        topic: Topic::keyed("files", root),
        read: Read::Git {
            root: root.into(),
            query: beep_proto::node::events::GitRead::State,
        },
    };
    let mut snapshots = Snapshots::new();
    snapshots.replace(&offers, &[], std::slice::from_ref(&query), &BTreeMap::new());
    let key = (query.topic.clone(), Some(1));

    for gap in [1, 2, 4, 8, 8].map(|doublings| READ_GAP * doublings) {
        let started = Instant::now();
        snapshots.start(&offers, caller);
        assert!(snapshots.entries[&key].running);
        assert!(matches!(
            stepped_snapshot(&mut snapshots).await,
            Some(Reply::SnapshotError { query: Some(1), .. })
        ));

        snapshots.changed(&query.topic, ChangeKind::Any);
        assert_eq!(snapshots.due(), Some(started + gap));
        tokio::time::advance(gap - Duration::from_millis(1)).await;
        snapshots.start(&offers, caller);
        assert!(snapshots.reads.is_empty());
        tokio::time::advance(Duration::from_millis(1)).await;
    }

    let value = Value::Plugin(vec![1]);
    let generation = snapshots.generation;
    let completed = key.clone();
    snapshots.entries.get_mut(&key).unwrap().running = true;
    snapshots.reads.spawn(async move {
        Completed {
            generation,
            key: completed,
            result: Ok((value, 1)),
        }
    });
    assert!(matches!(
        stepped_snapshot(&mut snapshots).await,
        Some(Reply::Snapshot(_))
    ));
    assert_eq!(snapshots.entries[&key].gap(), READ_GAP);
}
