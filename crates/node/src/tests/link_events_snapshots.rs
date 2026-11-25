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
        Value::Sessions(Box::new(SessionsReply::Processes {
            processes: vec![],
            containers: vec![],
        }))
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
            pages: vec![],
        }],
        containers: vec![],
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

#[tokio::test]
async fn one_sessions_process_change_leaves_other_sessions_clean() {
    let offers = Offers::default();
    let caller = iroh::SecretKey::generate().public();
    let (a, b) = (
        Topic::keyed("processes", "session-a"),
        Topic::keyed("processes", "session-b"),
    );
    let mut snapshots = Snapshots::new();
    snapshots.replace(&offers, &[a.clone(), b.clone()], &[], &BTreeMap::new());
    snapshots.start(&offers, caller);

    for _ in 0..2 {
        assert!(stepped_snapshot(&mut snapshots).await.is_some());
    }

    let dirty = |snapshots: &Snapshots, topic: &Topic| {
        snapshots.entries.get(&(topic.clone(), None)).unwrap().dirty
    };
    assert!(!dirty(&snapshots, &a) && !dirty(&snapshots, &b));

    snapshots.changed(&a, ChangeKind::Any);
    assert!(dirty(&snapshots, &a));
    assert!(!dirty(&snapshots, &b));
    // A subscriber to every session's processes still hears each one's change.
    assert!(affected(&Topic::new("processes"), &a));
    assert!(affected(&Topic::new("processes"), &b));
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
            source: Source::Plugin,
            previous: Some(value.clone()),
            bytes: 1,
            dirty: false,
            running: true,
            not_before: Instant::now(),
            idle: 0,
            sent: 0,
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
    let projection = |idle: u32| Projection {
        source: source.clone(),
        previous: None,
        bytes: 0,
        dirty: false,
        running: false,
        not_before: Instant::now(),
        idle,
        sent: 0,
    };
    assert_eq!(projection(0).gap(), READ_GAP);
    assert_eq!(projection(1).gap(), READ_GAP * 2);
    assert_eq!(projection(50).gap(), READ_GAP * 8);
    let list = Projection {
        source: Source::Read(Read::Sessions(Box::new(SessionsRequest::ListAll {}))),
        ..projection(50)
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

fn sources(topics: &[Topic]) -> Vec<(Topic, Source)> {
    let mut snapshots = Snapshots::new();
    snapshots.replace(&Offers::default(), topics, &[], &BTreeMap::new());
    snapshots
        .entries
        .into_iter()
        .map(|((topic, _), entry)| (topic, entry.source))
        .collect()
}

#[test]
fn a_released_readers_topics_get_what_a_released_node_gave() {
    let all = sources(&[Topic::new("*")]);

    assert!(all.iter().all(|(topic, _)| {
        !matches!(topic.name(), "devices" | "device-list" | "tool-updates")
    }));
    // A released node had no source for `devices`, and a released reader
    // cannot decode the list.
    assert!(sources(&[Topic::new("devices")]).is_empty());
    assert_eq!(
        sources(&[Topic::new("device-list"), Topic::new("tool-updates")])
            .into_iter()
            .map(|(_, source)| source)
            .collect::<Vec<_>>(),
        vec![Source::Devices, Source::ToolUpdates]
    );
}

#[test]
fn a_device_ping_refreshes_the_device_list() {
    let topic = Topic::new("device-list");
    let mut snapshots = Snapshots::new();
    snapshots.replace(&Offers::default(), std::slice::from_ref(&topic), &[], &BTreeMap::new());
    let key = (topic, None);
    snapshots.entries.get_mut(&key).unwrap().dirty = false;

    snapshots.changed(&Topic::new("devices"), ChangeKind::Any);

    assert!(snapshots.entries[&key].dirty);
}

#[tokio::test]
async fn tool_updates_are_missing_until_the_node_has_checked() {
    let topic = Topic::new("tool-updates");
    let mut snapshots = Snapshots::new();
    snapshots.replace(&Offers::default(), std::slice::from_ref(&topic), &[], &BTreeMap::new());
    snapshots.start(&Offers::default(), iroh::SecretKey::generate().public());

    let Some(Reply::Snapshot(snapshot)) = stepped_snapshot(&mut snapshots).await else {
        panic!("expected a snapshot");
    };
    assert_eq!((snapshot.topic, snapshot.value), (topic, Value::Missing));
}

#[test]
fn a_terminals_output_does_not_read_the_terminal_list_again() {
    let output = Topic::keyed("terminal-output", "7");

    assert!(!affected(&Topic::new("terminals"), &output));
    assert!(affected(&Topic::new("terminal-output"), &output));
    assert!(affected(&output, &output));
    assert!(!affected(&Topic::keyed("terminal-output", "8"), &output));
    // A start or an end still does.
    assert!(affected(&Topic::new("terminals"), &Topic::new("terminals")));
}

#[test]
fn a_projects_team_cards_have_a_source_of_their_own() {
    let topic = Topic::keyed(TEAM_CARDS_TOPIC, "org/repo");

    assert_eq!(
        sources(std::slice::from_ref(&topic)),
        vec![(topic.clone(), Source::TeamCards("org/repo".to_owned()))]
    );
    // Not for `*`, and not for every project at once.
    assert!(sources(&[Topic::new("*")]).iter().all(|(topic, _)| topic.name() != TEAM_CARDS_TOPIC));
    assert!(sources(&[Topic::new(TEAM_CARDS_TOPIC)]).is_empty());
    // A ring of the project's topic reads the cards again.
    assert!(affected(&topic, &topic));
}

#[test]
fn the_invites_have_a_source_of_their_own() {
    let topic = Topic::new(TEAM_INVITES_TOPIC);

    assert_eq!(
        sources(std::slice::from_ref(&topic)),
        vec![(topic.clone(), Source::TeamInvites)]
    );
    // Not for `*`.
    assert!(sources(&[Topic::new("*")]).iter().all(|(topic, _)| topic.name() != TEAM_INVITES_TOPIC));
    // A ring of the topic reads the invites again.
    assert!(affected(&topic, &topic));
}

#[test]
fn a_topic_naming_one_terminal_reads_that_terminal_alone() {
    use beep_proto::node::process::TaskId;

    let one = Topic::keyed("terminal-output", "7");

    assert_eq!(
        sources(std::slice::from_ref(&one)),
        vec![(one.clone(), Source::Terminal(TaskId(7)))]
    );
    // A desktop from before follows every terminal's output and still gets the list.
    let every = Topic::new("terminal-output");
    assert_eq!(
        sources(std::slice::from_ref(&every)),
        vec![(Topic::new("terminals"), Source::Read(Read::TerminalList))]
    );
    // A terminal starting or ending reads that terminal again, and so does its own output.
    assert!(affected(&one, &Topic::new("terminals")));
    assert!(affected(&one, &one));
}

#[test]
fn a_topic_naming_one_task_reads_that_task_alone() {
    use beep_proto::node::process::TaskId;

    let one = Topic::keyed("process-output", "3");

    assert_eq!(
        sources(std::slice::from_ref(&one)),
        vec![(one.clone(), Source::Task(TaskId(3)))]
    );
    // A desktop from before follows every task's output and still gets the list.
    let every = Topic::new("process-output");
    assert_eq!(
        sources(std::slice::from_ref(&every)),
        vec![(
            Topic::new("processes"),
            Source::Read(Read::Process(beep_proto::node::process::Request::List {}))
        )]
    );
    // A task starting or ending reads that task again, and so does any task's output.
    assert!(affected(&one, &Topic::new("processes")));
    assert!(affected(&one, &Topic::new("process-output")));
}

fn agents_snapshots(deltas: bool) -> Snapshots {
    let mut snapshots = Snapshots::new();
    snapshots.follow_deltas(deltas);
    snapshots.replace(
        &Offers::default(),
        &[Topic::new("agents")],
        &[],
        &BTreeMap::new(),
    );
    snapshots
}

fn list(sessions: Vec<SessionInfo>) -> Value {
    Value::Sessions(Box::new(SessionsReply::Sessions { sessions }))
}

/// Hands `snapshots` what a read of the `agents` list found, as the read does
/// before it is encoded, and returns what it sends.
async fn heard(snapshots: &mut Snapshots, value: Value) -> Option<Reply> {
    let key = (Topic::new("agents"), None);
    let value = shown(&key, value);
    let bytes = rmp_serde::to_vec(&value).unwrap().len();
    let generation = snapshots.generation;
    snapshots.entries.get_mut(&key).unwrap().running = true;
    snapshots.reads.spawn(async move {
        Completed {
            generation,
            key,
            result: Ok((value, bytes)),
        }
    });

    stepped_snapshot(snapshots).await
}

/// A session of the node at second `tick` of a long turn. The first twelve
/// run: each writes a longer message, takes on context and tokens, touches
/// files, and starts a new tool call every few seconds.
fn session_at(n: u64, tick: u64) -> SessionInfo {
    let running = n < 8;
    let tick = if running { tick } else { 0 };
    let mut body = String::from("I will look at the code first.\n");

    while body.len() < usize::try_from(tick * 60).unwrap().min(4096) {
        body.push_str("Reading the module and its callers to see what changes. ");
    }

    SessionInfo {
        id: format!("session{n}").as_str().into(),
        agent: "claude".into(),
        cwd: "/srv/project".into(),
        title: Some(format!("Task {n}")),
        started_at: 1_000 + n,
        state: beep_proto::node::agent::State {
            status: if running {
                beep_proto::node::agent::Status::Working
            } else {
                beep_proto::node::agent::Status::Idle
            },
            summary: if running {
                format!("Edit file{}.rs", tick / (3 + n % 4))
            } else {
                String::new()
            },
            files_touched: (0..(tick / 3).min(40)).map(|f| format!("src/file{f}.rs")).collect(),
            ..Default::default()
        },
        last_message: body,
        context: Some(beep_proto::AgentContext {
            used: tick * 1_000,
            size: 200_000,
        }),
        tokens: tick * 1_500,
        cost_usd: Some(tick as f64 / 100.0),
        last_activity: 2_000 + n,
        ..Default::default()
    }
}

fn sessions_at(tick: u64) -> Vec<SessionInfo> {
    (0..12).map(|n| session_at(n, tick)).collect()
}

/// Per-turn counters and text change on every update of a running turn; a list
/// that carried them was read and sent again each time.
#[tokio::test]
async fn what_changes_during_a_turn_does_not_change_the_session_list() {
    let mut snapshots = agents_snapshots(false);

    assert!(matches!(
        heard(&mut snapshots, list(sessions_at(10))).await,
        Some(Reply::Snapshot(_))
    ));
    // Only the counters, the touched files and the rest of the message grew.
    let mut grown = sessions_at(10);
    for info in &mut grown {
        info.tokens += 500;
        info.context = Some(beep_proto::AgentContext { used: 1, size: 2 });
        info.cost_usd = Some(9.0);
        info.state.files_touched.push("src/new.rs".into());
        info.last_message.push_str(" More words after the first line.");
    }
    assert!(heard(&mut snapshots, list(grown)).await.is_none());

    // What a row shows still sends.
    let mut doing = sessions_at(10);
    doing[0].state.summary = "Run the tests".into();
    assert!(matches!(
        heard(&mut snapshots, list(doing)).await,
        Some(Reply::Snapshot(_))
    ));
}

#[tokio::test]
async fn a_follower_of_deltas_gets_the_changed_rows_after_the_whole_list() {
    let mut snapshots = agents_snapshots(true);
    let first = list(sessions_at(10));
    let Some(Reply::Snapshot(whole)) = heard(&mut snapshots, first).await else {
        panic!("the first message is the whole list");
    };
    assert!(matches!(whole.value, Value::Sessions(_)));

    let mut next = sessions_at(10);
    next[1].state.summary = "Run the tests".into();
    next.remove(2);
    let Some(Reply::Snapshot(delta)) = heard(&mut snapshots, list(next)).await else {
        panic!("a change sends a delta");
    };
    let Value::SessionsDelta(delta) = delta.value else {
        panic!("a delta, not the list");
    };
    assert_eq!(delta.after, whole.revision, "it says which message it follows");
    assert_eq!(delta.changed.len(), 1);
    assert_eq!(delta.changed[0].id.to_string(), "session1");
    assert_eq!(delta.removed.len(), 1);
    assert_eq!(delta.removed[0].to_string(), "session2");

    // A refresh, a lost message: the list comes whole again, never a gap.
    snapshots.reset();
    let mut after = sessions_at(10);
    after[3].state.summary = "Read the logs".into();
    assert!(matches!(
        heard(&mut snapshots, list(after)).await,
        Some(Reply::Snapshot(whole)) if matches!(whole.value, Value::Sessions(_))
    ));
}

/// A desktop up to 0.10.33 sends no flag and reads no delta.
#[tokio::test]
async fn a_follower_that_asked_for_no_deltas_always_gets_the_whole_list() {
    let mut snapshots = agents_snapshots(false);
    heard(&mut snapshots, list(sessions_at(10))).await;
    let mut next = sessions_at(10);
    next[1].state.summary = "Run the tests".into();

    assert!(matches!(
        heard(&mut snapshots, list(next)).await,
        Some(Reply::Snapshot(whole)) if matches!(whole.value, Value::Sessions(_))
    ));
}

/// The bytes sent on `agents` while twelve sessions, eight of them running,
/// go through a minute of a turn, an update each second.
#[tokio::test]
async fn a_minute_of_running_sessions_sends_far_fewer_agents_bytes() {
    async fn sent(deltas: bool, slim: bool) -> usize {
        let mut snapshots = agents_snapshots(deltas);
        let key = (Topic::new("agents"), None);
        let mut total = 0;

        for tick in 1..=60 {
            let value = list(sessions_at(tick));
            let value = if slim { shown(&key, value) } else { value };
            let bytes = rmp_serde::to_vec(&value).unwrap().len();
            let generation = snapshots.generation;
            let completed = key.clone();
            snapshots.entries.get_mut(&key).unwrap().running = true;
            snapshots.reads.spawn(async move {
                Completed {
                    generation,
                    key: completed,
                    result: Ok((value, bytes)),
                }
            });

            if let Some(reply) = stepped_snapshot(&mut snapshots).await {
                total += rmp_serde::to_vec(&reply).unwrap().len();
            }
        }

        total
    }

    let before = sent(false, false).await;
    let rows_only = sent(false, true).await;
    let after = sent(true, true).await;
    println!("agents bytes in a minute: before {before}, rows only {rows_only}, after {after}");

    assert!(after * 10 < before, "before {before}, after {after}");
}

#[test]
fn only_the_agents_list_is_cut_to_rows() {
    let whole = list(sessions_at(10));
    let agents = (Topic::new("agents"), None);

    assert_ne!(shown(&agents, whole.clone()), whole);
    // A query for the same read, and the keyed topic of one session, stay whole.
    assert_eq!(shown(&(Topic::new("agents"), Some(1)), whole.clone()), whole);
    assert_eq!(shown(&(Topic::keyed("agent", "session1"), None), whole.clone()), whole);
}
