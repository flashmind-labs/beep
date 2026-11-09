use super::*;
use beep_proto::node::{
    events::{Query, Read, Value},
    local_link, process,
};

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
        matches!(snapshot.value, Value::Sessions(reply) if matches!(*reply, SessionsReply::Tree { ref tree } if tree.folders.iter().any(|folder| folder.entries.iter().any(|entry| entry.name == "example.txt"))))
    );
}
