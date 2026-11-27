use super::*;
use beep_proto::node::{
    agent::SessionsReply,
    events::{CallReply, Operation, Query, Read, Reply, Value},
    local_link, process,
};
use tokio_util::task::AbortOnDropHandle;

async fn next(rx: &mut LinkRx) -> Reply {
    loop {
        let reply = timeout(Duration::from_secs(10), rx.recv::<Reply>())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if !matches!(reply, Reply::Pong { id: 0 }) {
            return reply;
        }
    }
}

async fn start(offers: Offers) -> (LinkTx, LinkRx, AbortOnDropHandle<Result<()>>) {
    let link = local_link();
    let task = AbortOnDropHandle::new(tokio::spawn(serve(
        link.node.0,
        link.node.1,
        offers,
        iroh::SecretKey::generate().public(),
    )));
    let (tx, mut rx) = link.caller;
    assert_eq!(next(&mut rx).await, Reply::Ready);
    (tx, rx, task)
}

async fn watch(tx: &mut LinkTx, rx: &mut LinkRx, id: u64, topics: Vec<Topic>, queries: Vec<Query>) {
    tx.send(&Request::Watch {
        id,
        topics,
        queries,
    })
    .await
    .unwrap();

    loop {
        if matches!(next(rx).await, Reply::Watching { id: ack } if ack == id) {
            break;
        }
    }
}

async fn state(rx: &mut LinkRx, topic: &Topic) -> events::Snapshot {
    loop {
        if let Reply::Snapshot(snapshot) = next(rx).await
            && &snapshot.topic == topic
        {
            return *snapshot;
        }
    }
}

#[tokio::test]
async fn snapshots_and_correlated_commands_share_one_connection() {
    let offers = Offers::default();
    let topic = Topic::keyed("plugin", "example");
    offers
        .agents
        .events
        .publish(topic.clone(), vec![1])
        .unwrap();
    let hub = offers.agents.events.clone();
    let (mut tx, mut rx, _task) = start(offers).await;
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    assert_eq!(state(&mut rx, &topic).await.value, Value::Plugin(vec![1]));
    hub.publish(topic.clone(), vec![2]).unwrap();
    tx.send(&Request::Call {
        id: 2,
        operation: Box::new(Operation::Process(process::Request::List {})),
    })
    .await
    .unwrap();
    tx.send(&Request::Ping { id: 3 }).await.unwrap();
    let (mut snapshot, mut call, mut pong) = (false, false, false);

    while !(snapshot && call && pong) {
        match next(&mut rx).await {
            Reply::Snapshot(s) if s.topic == topic => {
                assert_eq!(s.value, Value::Plugin(vec![2]));
                snapshot = true;
            }
            Reply::Result { id: 2, reply } => {
                assert!(matches!(
                    *reply,
                    CallReply::Process(process::Reply::Tasks { .. })
                ));
                call = true;
            }
            Reply::Pong { id: 3 } => pong = true,
            other => panic!("unexpected reply: {other:?}"),
        }
    }
}

#[tokio::test]
async fn sign_in_topics_send_current_state_without_starting_a_login() {
    let (mut tx, mut rx, _task) = start(Offers::default()).await;
    let topic = Topic::keyed("sign-in", "codex");
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    assert_eq!(
        state(&mut rx, &topic).await.value,
        Value::SignIn(beep_proto::node::sign_in::State::Idle {})
    );
}

#[tokio::test]
async fn refresh_sends_current_state_even_when_it_has_not_changed() {
    let offers = Offers::default();
    let topic = Topic::keyed("plugin", "example");
    offers
        .agents
        .events
        .publish(topic.clone(), vec![1])
        .unwrap();
    let (mut tx, mut rx, _task) = start(offers).await;
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    let initial = state(&mut rx, &topic).await;
    tx.send(&Request::Refresh { id: 2 }).await.unwrap();
    assert_eq!(next(&mut rx).await, Reply::Watching { id: 2 });
    let refreshed = state(&mut rx, &topic).await;
    assert_eq!(refreshed.value, initial.value);
    assert!(refreshed.revision > initial.revision);
}

#[tokio::test]
async fn reconnect_reads_latest_state_without_replaying_intermediate_updates() {
    let offers = Offers::default();
    let topic = Topic::keyed("plugin", "example");
    offers
        .agents
        .events
        .publish(topic.clone(), vec![1])
        .unwrap();
    let (mut tx, mut rx, task) = start(offers.clone()).await;
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    state(&mut rx, &topic).await;
    drop(task);
    drop(tx);
    drop(rx);
    offers
        .agents
        .events
        .publish(topic.clone(), vec![2])
        .unwrap();
    offers
        .agents
        .events
        .publish(topic.clone(), vec![3])
        .unwrap();
    let (mut tx, mut rx, _task) = start(offers).await;
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    assert_eq!(state(&mut rx, &topic).await.value, Value::Plugin(vec![3]));
}

#[tokio::test]
async fn adding_queries_preserves_default_snapshots_and_rejects_mutations() {
    let offers = Offers::default();
    let topic = Topic::keyed("plugin", "example");
    offers
        .agents
        .events
        .publish(topic.clone(), vec![1])
        .unwrap();
    let (mut tx, mut rx, _task) = start(offers).await;
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    state(&mut rx, &topic).await;
    let process_topic = Topic::new("processes");
    watch(
        &mut tx,
        &mut rx,
        2,
        vec![topic.clone()],
        vec![Query {
            id: 7,
            topic: process_topic.clone(),
            read: Read::Process(process::Request::List {}),
        }],
    )
    .await;
    let snapshot = state(&mut rx, &process_topic).await;
    assert_eq!(snapshot.query, Some(7));
    tx.send(&Request::Watch {
        id: 3,
        topics: vec![],
        queries: vec![Query {
            id: 8,
            topic: process_topic,
            read: Read::Process(process::Request::Start {
                command: "false".into(),
                cwd: None,
            }),
        }],
    })
    .await
    .unwrap();

    loop {
        match next(&mut rx).await {
            Reply::Error { id: 3, .. } => break,
            Reply::Snapshot(_) => {}
            other => panic!("unexpected invalid-query response: {other:?}"),
        }
    }
    // The rejected Watch leaves the previous subscription live.
    tx.send(&Request::Ping { id: 4 }).await.unwrap();

    loop {
        match next(&mut rx).await {
            Reply::Pong { id: 4 } => break,
            Reply::Snapshot(_) => {}
            other => panic!("unexpected ping response: {other:?}"),
        }
    }
}

#[tokio::test]
async fn file_snapshots_include_new_entries_and_enforce_reach() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let offers = Offers::default();
    offers.agents.share_folders(vec![root.path().to_owned()]);
    let (mut tx, mut rx, _task) = start(offers).await;
    tx.send(&Request::Watch {
        id: 1,
        topics: vec![Topic::keyed("files", outside.path().to_str().unwrap())],
        queries: vec![],
    })
    .await
    .unwrap();
    assert!(matches!(next(&mut rx).await, Reply::Error { id: 1, .. }));
    let topic = Topic::keyed("files", root.path().to_str().unwrap());
    watch(&mut tx, &mut rx, 2, vec![topic.clone()], vec![]).await;
    state(&mut rx, &topic).await;
    std::fs::write(root.path().join("example.txt"), "updated").unwrap();
    let snapshot = state(&mut rx, &topic).await;
    assert!(
        matches!(snapshot.value, Value::Sessions(reply) if matches!(*reply, SessionsReply::Tree { ref tree, .. } if tree.folders.iter().any(|folder| folder.entries.iter().any(|entry| entry.name == "example.txt"))))
    );
}

// The first snapshots and the bind can say "anything changed". Once they are past, a write is
// named by its path.
#[tokio::test]
async fn a_folders_snapshot_names_the_paths_that_changed_since_the_one_before() {
    let root = tempfile::tempdir().unwrap();
    let offers = Offers::default();
    offers.agents.share_folders(vec![root.path().to_owned()]);
    let (mut tx, mut rx, _task) = start(offers).await;
    let topic = Topic::keyed("files", root.path().to_str().unwrap());
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    let first = state(&mut rx, &topic).await;
    assert!(
        matches!(first.value, Value::Sessions(reply) if matches!(*reply, SessionsReply::Tree { ref changed, .. } if changed.is_empty())),
        "a first snapshot does not say what changed"
    );

    for round in 0..50 {
        let name = format!("named-{round}.txt");
        std::fs::write(root.path().join(&name), "x").unwrap();
        let snapshot = state(&mut rx, &topic).await;
        let Value::Sessions(reply) = snapshot.value else {
            panic!("a folder is a listing");
        };
        let SessionsReply::Tree { changed, .. } = *reply else {
            panic!("a folder is a listing");
        };

        if !changed.is_empty() {
            assert!(
                changed.iter().all(|path| path.starts_with("named-")),
                "{changed:?}"
            );
            return;
        }
    }

    panic!("no snapshot named a changed path");
}

#[tokio::test]
async fn a_blocked_topic_stream_does_not_block_another_subscription() {
    let offers = Offers::default();
    let large = Topic::keyed("plugin", "large");
    let small = Topic::keyed("plugin", "small");
    // Exceeds the in-process stream buffer so the first writer must wait.
    offers
        .agents
        .events
        .publish(large.clone(), vec![7; 256 * 1024])
        .unwrap();
    offers
        .agents
        .events
        .publish(small.clone(), vec![1])
        .unwrap();
    let (mut large_tx, mut large_rx, _large_task) = start(offers.clone()).await;
    watch(&mut large_tx, &mut large_rx, 1, vec![large], vec![]).await;

    let (mut small_tx, mut small_rx, _small_task) = start(offers.clone()).await;
    watch(&mut small_tx, &mut small_rx, 1, vec![small.clone()], vec![]).await;
    assert_eq!(
        state(&mut small_rx, &small).await.value,
        Value::Plugin(vec![1])
    );

    drop(large_rx);
    drop(large_tx);
    offers
        .agents
        .events
        .publish(small.clone(), vec![2])
        .unwrap();
    assert_eq!(
        state(&mut small_rx, &small).await.value,
        Value::Plugin(vec![2])
    );
}

// Binding the file watchers can take long (Git, the process-wide watcher).
// The first snapshot and the acknowledgement must not wait for it.
#[tokio::test]
async fn a_watch_is_answered_while_its_watchers_are_still_binding() {
    let root = tempfile::tempdir().unwrap();
    let offers = Offers::default();
    offers.agents.share_folders(vec![root.path().to_owned()]);
    let (mut tx, mut rx, _task) = start(offers.clone()).await;
    let (reached, reached_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release) = tokio::sync::oneshot::channel();
    // Resolved the way the node does, so the gate matches the bound root.
    let real = root.path().canonicalize().unwrap();
    crate::lock(&offers.bind_gates).push(sources::Gate {
        root: real,
        reached,
        release,
    });
    let topic = Topic::keyed("files", root.path().to_str().unwrap());
    watch(&mut tx, &mut rx, 1, vec![topic.clone()], vec![]).await;
    let snapshot = state(&mut rx, &topic).await;
    assert!(matches!(snapshot.value, Value::Sessions(_)));
    reached_rx.await.expect("the bind started and is held");

    // Released, the watcher binds and a later change is seen.
    release_tx.send(()).unwrap();
    std::fs::write(root.path().join("later.txt"), "x").unwrap();
    let snapshot = state(&mut rx, &topic).await;
    assert!(
        matches!(snapshot.value, Value::Sessions(reply) if matches!(*reply, SessionsReply::Tree { ref tree, .. } if tree.folders.iter().any(|folder| folder.entries.iter().any(|entry| entry.name == "later.txt"))))
    );
}

// A watch can wait on a slow root check. A ping sent behind it is answered
// at once, and a second watch is answered after the first.
#[tokio::test]
async fn a_ping_is_answered_while_a_watch_is_looked_up_and_watches_keep_their_order() {
    let root = tempfile::tempdir().unwrap();
    let offers = Offers::default();
    offers.agents.share_folders(vec![root.path().to_owned()]);
    let (mut tx, mut rx, _task) = start(offers.clone()).await;
    let topic = Topic::keyed("files", root.path().to_str().unwrap());
    // Holding the registry stops every source lookup, so the watches wait.
    let registry = offers.event_sources.lock().await;

    for id in [1, 2] {
        tx.send(&Request::Watch {
            id,
            topics: vec![topic.clone()],
            queries: vec![],
        })
        .await
        .unwrap();
    }
    tx.send(&Request::Ping { id: 7 }).await.unwrap();
    assert_eq!(next(&mut rx).await, Reply::Pong { id: 7 });

    drop(registry);
    let mut acknowledged = Vec::new();

    while acknowledged.len() < 2 {
        if let Reply::Watching { id } = next(&mut rx).await {
            acknowledged.push(id);
        }
    }

    assert_eq!(acknowledged, [1, 2]);
}

mod node_caller {
    use super::*;
    use beep_proto::node::{
        events::FORGE_REACH_TOPIC,
        subscriptions::{Reply, Request},
    };

    async fn reply(rx: &mut LinkRx) -> Reply {
        loop {
            let reply = timeout(Duration::from_secs(10), rx.recv::<Reply>())
                .await
                .unwrap()
                .unwrap()
                .unwrap();

            if !matches!(reply, Reply::Pong { .. }) {
                return reply;
            }
        }
    }

    fn watch(id: u64, topic: Topic) -> Request {
        Request::Watch {
            id,
            topic,
            queries: Vec::new(),
            deltas: false,
            query_deltas: false,
        }
    }

    #[tokio::test]
    async fn a_node_is_refused_every_topic_but_agents_and_forge_reach() {
        let offers = Offers::default();
        let _events = offers.agents.events.forward_system();
        let link = local_link();
        let _task = AbortOnDropHandle::new(tokio::spawn(serve_subscriptions(
            link.node.0,
            link.node.1,
            offers.clone(),
            iroh::SecretKey::generate().public(),
            Reach::Node,
            false,
        )));
        let (mut tx, mut rx) = link.caller;
        assert!(matches!(reply(&mut rx).await, Reply::Ready));

        for (id, topic) in [
            (1, Topic::keyed("files", "/srv/project")),
            (2, Topic::new("terminals")),
            (3, Topic::keyed("agent", "s1")),
            (4, Topic::new("*")),
        ] {
            tx.send(&watch(id, topic)).await.unwrap();
            assert!(
                matches!(reply(&mut rx).await, Reply::Error { id: refused, .. } if refused == id)
            );
        }

        tx.send(&watch(5, Topic::new(FORGE_REACH_TOPIC)))
            .await
            .unwrap();
        let mut said = false;

        // How a host is reached is pushed when it changes, not asked for.
        while !said {
            match reply(&mut rx).await {
                Reply::Snapshot(snapshot) => {
                    if let Value::ForgeReach(reach) = snapshot.value {
                        said = reach
                            .hosts
                            .get("github.com")
                            .is_some_and(|host| host.reached);
                    }

                    if !said {
                        offers.agents.ci.record_reach("github.com", true);
                    }
                }
                Reply::Error { reason, .. } => panic!("{reason}"),
                _ => {}
            }
        }
    }
}
