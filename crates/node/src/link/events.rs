//! An authenticated peer can replace topics and send commands while updates arrive.

use super::Offers;
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{SessionsReply, SessionsRequest},
    events::{self, CallReply, Operation, Reply, Request, Topic},
};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::broadcast::error::RecvError,
    task::JoinSet,
    time::{interval, timeout},
};
use tokio_util::task::AbortOnDropHandle;

const WRITE_WAIT: Duration = Duration::from_secs(15);
const MAX_CALLS: usize = 16;

pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    offers.agents.watch_event_sources().await?;
    let mut changes = offers.agents.events.subscribe();
    let mut topics: Vec<Topic> = Vec::new();
    let mut considered = offers.agents.events.cursor();
    let roots = Arc::new(Mutex::new(BTreeMap::<String, PathBuf>::new()));
    let changed_roots = roots.clone();
    let hub = offers.agents.events.clone();
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                for key in crate::lock(&changed_roots).keys() {
                    hub.changed(Topic::keyed("files", key));
                }
                return;
            };
            if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
                return;
            }
            for (key, root) in crate::lock(&changed_roots).iter() {
                if event.paths.iter().any(|path| {
                    path.starts_with(root)
                        && !path.strip_prefix(root).is_ok_and(|path| {
                            path.components().any(|part| {
                                matches!(
                                    part.as_os_str().to_str(),
                                    Some("node_modules" | "target" | ".worktrees")
                                )
                            })
                        })
                }) {
                    hub.changed(Topic::keyed("files", key));
                }
            }
        },
        notify::Config::default(),
    )?;
    let mut calls = JoinSet::new();
    let mut pending = HashSet::new();
    let mut heartbeat = interval(Duration::from_secs(10));
    send(&mut tx, &Reply::Ready).await?;
    loop {
        tokio::select! {
            request = rx.recv::<Request>() => match request? {
                None => return Ok(()),
                Some(Request::Watch { id, topics: wanted, after }) => {
                    match watch(&offers, &mut watcher, &roots, &wanted).await {
                        Ok(()) => {
                            topics = wanted;
                            // Connection-owned file watchers were absent during a disconnect.
                            // Reconcile those resources as identified events before catching up.
                            if after.is_some() {
                                for topic in topics.iter().filter(|topic| topic.name == "files") {
                                    offers.agents.events.changed(topic.clone());
                                }
                            }
                            send(&mut tx, &Reply::Watching { id }).await?;
                            let replay = offers.agents.events.replay(after);
                            considered = replay.cursor;
                            match replay.events {
                                Some(events) => {
                                    for event in events { deliver(&mut tx, &topics, event).await?; }
                                    send(&mut tx, &Reply::CaughtUp { cursor: considered }).await?;
                                }
                                _ => send(&mut tx, &Reply::Resync { cursor: considered }).await?,
                            }
                        }
                        Err(error) => send(&mut tx, &Reply::Error { id, reason: format!("{error:#}") }).await?,
                    }
                }
                Some(Request::Ping { id }) => send(&mut tx, &Reply::Pong { id }).await?,
                Some(Request::Call { id, operation }) => {
                    if calls.len() >= MAX_CALLS || !pending.insert(id) {
                        send(&mut tx, &Reply::Error { id, reason: "too many pending calls or duplicate call id".into() }).await?;
                    } else {
                        let agents = offers.agents.clone();
                        calls.spawn(async move { (id, timeout(Duration::from_secs(120), execute(agents, caller, *operation)).await) });
                    }
                }
            },
            Some(result) = calls.join_next(), if !calls.is_empty() => {
                let (id, result) = result.context("a node command stopped")?;
                pending.remove(&id);
                let reply = match result {
                    Ok(Ok(reply)) => Reply::Result { id, reply: Box::new(reply) },
                    Ok(Err(error)) => Reply::Error { id, reason: format!("{error:#}") },
                    Err(_) => Reply::Error { id, reason: "command timed out; its outcome may be unknown".into() },
                };
                send(&mut tx, &reply).await?;
            },
            event = changes.recv() => {
                match event {
                    Ok(event) if event.id.epoch != considered.epoch || event.id.sequence > considered.sequence => {
                        considered = event.id;
                        deliver(&mut tx, &topics, event).await?;
                    }
                    Ok(_) => {},
                    Err(RecvError::Lagged(_)) => {
                        considered = offers.agents.events.cursor();
                        send(&mut tx, &Reply::Resync { cursor: considered }).await?;
                    }
                    Err(RecvError::Closed) => return Ok(()),
                }
            },
            _ = heartbeat.tick() => send(&mut tx, &Reply::CaughtUp { cursor: considered }).await?,
        }
    }
}

async fn send(tx: &mut LinkTx, reply: &Reply) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(reply))
        .await
        .context("event reader stopped receiving")?
}

async fn deliver(tx: &mut LinkTx, topics: &[Topic], event: events::Event) -> Result<()> {
    if event.topic.name == "*" {
        send(tx, &Reply::Resync { cursor: event.id }).await
    } else if topics.iter().any(|topic| topic.matches(&event.topic)) {
        send(tx, &Reply::Changed(event)).await
    } else {
        Ok(())
    }
}

async fn watch(
    offers: &Offers,
    watcher: &mut RecommendedWatcher,
    roots: &Mutex<BTreeMap<String, PathBuf>>,
    topics: &[Topic],
) -> Result<()> {
    if topics.len() > events::MAX_TOPICS || topics.iter().any(|topic| !topic.valid()) {
        bail!("invalid subscription topics");
    }
    let mut wanted = BTreeMap::new();
    for topic in topics.iter().filter(|topic| topic.name == "files") {
        if let Some(path) = &topic.key {
            let root = offers
                .agents
                .reachable(path, crate::agents::reach::Access::Read)
                .await?;
            wanted.insert(path.clone(), root);
        }
    }
    let mut held = crate::lock(roots);
    let mut added = Vec::new();
    for (key, root) in &wanted {
        if held.get(key) != Some(root) {
            if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
                for root in added {
                    let _ = watcher.unwatch(root);
                }
                return Err(error.into());
            }
            added.push(root.as_path());
        }
    }
    for (key, root) in held.iter() {
        if wanted.get(key) != Some(root) {
            let _ = watcher.unwatch(root);
        }
    }
    *held = wanted;
    Ok(())
}

async fn execute(
    agents: Arc<crate::agents::Agents>,
    caller: iroh::EndpointId,
    operation: Operation,
) -> Result<CallReply> {
    match operation {
        Operation::Agent {
            agent,
            session,
            request,
        } => {
            let info = agents
                .send_session(&agent, &session, request)
                .await
                .map_err(anyhow::Error::msg)?;
            Ok(CallReply::Agent {
                info: Box::new(info),
            })
        }
        Operation::Process(request) => Ok(CallReply::Process(
            crate::run::tasks::answer(crate::run::tasks::Table::Shared, request).await,
        )),
        Operation::Sessions(request) => {
            if matches!(
                request,
                SessionsRequest::Watch { .. }
                    | SessionsRequest::FollowPresence {}
                    | SessionsRequest::FollowImages {}
                    | SessionsRequest::FollowAlerts {}
                    | SessionsRequest::FollowRuns {}
                    | SessionsRequest::FollowUsage { .. }
            ) {
                bail!("use a topic subscription for a following request");
            }
            let link = beep_proto::node::local_link();
            let task = AbortOnDropHandle::new(tokio::spawn(crate::agents::serve_sessions(
                link.node.0,
                link.node.1,
                agents,
                caller,
            )));
            let (mut tx, mut rx) = link.caller;
            tx.send(&request).await?;
            let reply = rx
                .recv::<SessionsReply>()
                .await?
                .context("session request closed without a reply")?;
            drop(task);
            Ok(CallReply::Sessions(Box::new(reply)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::{events::Event, local_link, process};

    async fn next(rx: &mut LinkRx) -> Reply {
        loop {
            let reply = timeout(Duration::from_secs(5), rx.recv::<Reply>())
                .await
                .expect("the stream answers")
                .expect("a valid reply")
                .expect("the stream stays open");
            if !matches!(reply, Reply::Pong { id: 0 } | Reply::CaughtUp { .. }) {
                return reply;
            }
        }
    }

    async fn subscribe(tx: &mut LinkTx, rx: &mut LinkRx, id: u64, topics: Vec<Topic>) {
        tx.send(&Request::Watch {
            id,
            topics,
            after: None,
        })
        .await
        .unwrap();
        assert_eq!(next(rx).await, Reply::Watching { id });
        assert!(matches!(next(rx).await, Reply::Resync { .. }));
    }

    #[tokio::test]
    async fn topics_and_correlated_calls_share_one_connection() {
        let offers = Offers::default();
        let hub = offers.agents.events.clone();
        let link = local_link();
        let task = AbortOnDropHandle::new(tokio::spawn(serve(
            link.node.0,
            link.node.1,
            offers,
            iroh::SecretKey::generate().public(),
        )));
        let (mut tx, mut rx) = link.caller;
        assert_eq!(next(&mut rx).await, Reply::Ready);
        subscribe(
            &mut tx,
            &mut rx,
            1,
            vec![Topic::keyed("plugin.example", "one")],
        )
        .await;
        hub.changed(Topic::keyed("plugin.example", "other"));
        hub.changed(Topic::new("reviews"));
        hub.changed(Topic::keyed("plugin.example", "one"));
        let Reply::Changed(event) = next(&mut rx).await else {
            panic!("expected a selected event")
        };
        assert_eq!(event.topic, Topic::keyed("plugin.example", "one"));
        tx.send(&Request::Call {
            id: 8,
            operation: Box::new(Operation::Process(process::Request::List {})),
        })
        .await
        .unwrap();
        let Reply::Result { id, reply } = next(&mut rx).await else {
            panic!("expected a correlated reply")
        };
        assert_eq!(id, 8);
        assert!(matches!(
            *reply,
            CallReply::Process(process::Reply::Tasks { .. })
        ));
        subscribe(&mut tx, &mut rx, 2, vec![Topic::new("*")]).await;
        hub.changed(Topic::new("plugin.another"));
        assert!(
            matches!(next(&mut rx).await, Reply::Changed(Event { topic, .. }) if topic.name == "plugin.another")
        );
        subscribe(&mut tx, &mut rx, 3, Vec::new()).await;
        hub.changed(Topic::new("plugin.another"));
        tx.send(&Request::Ping { id: 99 }).await.unwrap();
        assert_eq!(next(&mut rx).await, Reply::Pong { id: 99 });
        drop(tx);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn reconnect_replays_selected_events_after_the_last_received_id_once() {
        let offers = Offers::default();
        let hub = offers.agents.events.clone();
        let caller = iroh::SecretKey::generate().public();
        let link = local_link();
        let task = AbortOnDropHandle::new(tokio::spawn(serve(
            link.node.0,
            link.node.1,
            offers.clone(),
            caller,
        )));
        let (mut tx, mut rx) = link.caller;
        assert_eq!(next(&mut rx).await, Reply::Ready);
        let topic = Topic::new("plugin.replay");
        subscribe(&mut tx, &mut rx, 1, vec![topic.clone()]).await;
        hub.changed(topic.clone());
        let Reply::Changed(first) = next(&mut rx).await else {
            panic!("first event")
        };
        drop(tx);
        drop(rx);
        task.await.unwrap().unwrap();
        hub.changed(topic.clone());
        let second = hub.cursor();
        hub.changed(Topic::new("plugin.unselected"));
        hub.changed(topic.clone());
        let third = hub.cursor();
        let link = local_link();
        let _task = AbortOnDropHandle::new(tokio::spawn(serve(
            link.node.0,
            link.node.1,
            offers,
            caller,
        )));
        let (mut tx, mut rx) = link.caller;
        assert_eq!(next(&mut rx).await, Reply::Ready);
        tx.send(&Request::Watch {
            id: 2,
            topics: vec![topic],
            after: Some(first.id),
        })
        .await
        .unwrap();
        assert_eq!(next(&mut rx).await, Reply::Watching { id: 2 });
        let Reply::Changed(event) = next(&mut rx).await else {
            panic!("replayed second event")
        };
        assert_eq!(event.id, second);
        let Reply::Changed(event) = next(&mut rx).await else {
            panic!("replayed third event")
        };
        assert_eq!(event.id, third);
        tx.send(&Request::Ping { id: 3 }).await.unwrap();
        assert_eq!(next(&mut rx).await, Reply::Pong { id: 3 });
    }

    #[tokio::test]
    async fn invalid_topics_leave_the_previous_subscription_in_place() {
        let offers = Offers::default();
        let hub = offers.agents.events.clone();
        let link = local_link();
        let _task = AbortOnDropHandle::new(tokio::spawn(serve(
            link.node.0,
            link.node.1,
            offers,
            iroh::SecretKey::generate().public(),
        )));
        let (mut tx, mut rx) = link.caller;
        assert_eq!(next(&mut rx).await, Reply::Ready);
        subscribe(&mut tx, &mut rx, 1, vec![Topic::new("reviews")]).await;
        tx.send(&Request::Watch {
            id: 2,
            topics: vec![Topic::new("")],
            after: None,
        })
        .await
        .unwrap();
        assert!(matches!(next(&mut rx).await, Reply::Error { id: 2, .. }));
        hub.changed(Topic::new("reviews"));
        assert!(matches!(next(&mut rx).await, Reply::Changed(_)));
    }

    #[tokio::test]
    async fn a_source_gap_is_a_snapshot_request_with_an_event_id() {
        let link = local_link();
        let (mut tx, _) = link.node;
        let (_, mut rx) = link.caller;
        let cursor = crate::events::Hub::default().cursor();
        deliver(
            &mut tx,
            &[],
            Event {
                topic: Topic::new("*"),
                id: cursor,
            },
        )
        .await
        .unwrap();
        assert_eq!(next(&mut rx).await, Reply::Resync { cursor });
    }

    #[tokio::test]
    async fn file_subscriptions_watch_real_changes_and_enforce_reach() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let offers = Offers::default();
        offers.agents.share_folders(vec![root.path().to_owned()]);
        let link = local_link();
        let _task = AbortOnDropHandle::new(tokio::spawn(serve(
            link.node.0,
            link.node.1,
            offers,
            iroh::SecretKey::generate().public(),
        )));
        let (mut tx, mut rx) = link.caller;
        assert_eq!(next(&mut rx).await, Reply::Ready);
        tx.send(&Request::Watch {
            id: 1,
            topics: vec![Topic::keyed("files", outside.path().to_str().unwrap())],
            after: None,
        })
        .await
        .unwrap();
        assert!(matches!(next(&mut rx).await, Reply::Error { id: 1, .. }));
        let topic = Topic::keyed("files", root.path().to_str().unwrap());
        subscribe(&mut tx, &mut rx, 2, vec![topic.clone()]).await;
        std::fs::write(root.path().join("example.txt"), "updated").unwrap();
        assert!(matches!(next(&mut rx).await, Reply::Changed(event) if event.topic == topic));
    }
}
