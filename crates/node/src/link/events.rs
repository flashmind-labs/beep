//! An authenticated peer can replace topics and send commands while updates arrive.

use super::Offers;
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{SessionsReply, SessionsRequest},
    events::{self, CallReply, Operation, Reply, Request, Topic},
};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::broadcast::error::RecvError,
    task::JoinSet,
    time::{Instant, MissedTickBehavior, interval_at, timeout},
};
use tokio_util::task::AbortOnDropHandle;

const WRITE_WAIT: Duration = Duration::from_secs(15);
const MAX_CALLS: usize = 16;
const MAX_FILE_ROOTS: usize = 512;

pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    offers.agents.start_event_bridge();
    let mut changes = offers.agents.events.subscribe();
    let mut topics: Vec<Topic> = Vec::new();
    let mut considered = offers.agents.events.cursor();
    let roots = Arc::new(Mutex::new(BTreeMap::<String, PathBuf>::new()));
    let mut watcher = None;
    let mut _source_lease = None;
    let mut calls = JoinSet::new();
    let mut pending = HashSet::new();
    let mut heartbeat = interval_at(
        Instant::now() + Duration::from_secs(10),
        Duration::from_secs(10),
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    send(&mut tx, &Reply::Ready).await?;
    loop {
        tokio::select! {
            request = rx.recv::<Request>() => match request? {
                None => return Ok(()),
                Some(Request::Watch { id, topics: wanted, after }) => {
                    match subscribe(&offers, &mut watcher, &roots, &wanted).await {
                        Ok(lease) => {
                            _source_lease = lease;
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
                        if broad_files(&topics) && matches!(event.topic.name.as_str(), "agents" | "projects" | "*")
                            && let Err(error) = watch(&offers, &mut watcher, &roots, &topics).await
                        {
                            log::debug!("file subscription roots could not be refreshed: {error}");
                            send(&mut tx, &Reply::Resync { cursor: event.id }).await?;
                        }
                        considered = event.id;
                        deliver(&mut tx, &topics, event).await?;
                    }
                    Ok(_) => {},
                    Err(RecvError::Lagged(_)) => {
                        if broad_files(&topics)
                            && let Err(error) = watch(&offers, &mut watcher, &roots, &topics).await
                        {
                            log::debug!("file subscription roots could not be reconciled: {error}");
                        }
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

fn broad_files(topics: &[Topic]) -> bool {
    topics
        .iter()
        .any(|topic| topic.name == "*" || (topic.name == "files" && topic.key.is_none()))
}

async fn subscribe(
    offers: &Offers,
    watcher: &mut Option<crate::events::paths::Paths>,
    roots: &Arc<Mutex<BTreeMap<String, PathBuf>>>,
    topics: &[Topic],
) -> Result<Option<Arc<crate::agents::follow::Sources>>> {
    if topics.len() > events::MAX_TOPICS || topics.iter().any(|topic| !topic.valid()) {
        bail!("invalid subscription topics");
    }
    let lease = if broad_files(topics)
        || topics
            .iter()
            .any(|topic| matches!(topic.name.as_str(), "agent" | "agents" | "*"))
    {
        Some(offers.agents.watch_event_sources().await?)
    } else {
        None
    };
    watch(offers, watcher, roots, topics).await?;
    Ok(lease)
}

async fn watch(
    offers: &Offers,
    watcher: &mut Option<crate::events::paths::Paths>,
    roots: &Arc<Mutex<BTreeMap<String, PathBuf>>>,
    topics: &[Topic],
) -> Result<()> {
    let mut wanted = BTreeMap::new();
    if broad_files(topics) {
        for root in offers.agents.notification_roots().await? {
            let key = root
                .to_str()
                .context("a notification root is not UTF-8")?
                .to_owned();
            wanted.insert(key, root);
        }
    }
    for topic in topics.iter().filter(|topic| topic.name == "files") {
        if let Some(path) = &topic.key {
            let root = offers
                .agents
                .reachable(path, crate::agents::reach::Access::Read)
                .await?;
            wanted.insert(path.clone(), root);
        }
    }
    if wanted.len() > MAX_FILE_ROOTS {
        bail!("too many watched directories; subscribe to specific file roots");
    }
    if *crate::lock(roots) == wanted {
        return Ok(());
    }
    if wanted.is_empty() {
        *watcher = None;
    } else {
        if watcher.is_none() {
            let changed_roots = roots.clone();
            let hub = offers.agents.events.clone();
            *watcher = Some(crate::events::paths::Paths::new(move |paths| {
                let held = crate::lock(&changed_roots);
                for (key, root) in held.iter() {
                    if paths.iter().any(|path| path == root) {
                        hub.changed(Topic::keyed("files", key));
                    }
                }
            })?);
        }
        if let Some(watcher) = watcher.as_ref() {
            watcher.replace(wanted.values().cloned().collect()).await?;
        }
    }
    // A root can appear or disappear while a broad subscription remains active.
    // Reconcile it after installing the new key mapping, including any watch gap.
    let previous = std::mem::replace(&mut *crate::lock(roots), wanted.clone());
    for key in previous.keys().chain(wanted.keys()).collect::<HashSet<_>>() {
        offers.agents.events.changed(Topic::keyed("files", key));
    }
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
        loop {
            match next(rx).await {
                Reply::Changed(_) => continue,
                reply => {
                    assert_eq!(reply, Reply::Watching { id });
                    break;
                }
            }
        }
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
    async fn broad_files_watch_shared_roots_and_alias_removal_keeps_the_watch() {
        let root = tempfile::tempdir().unwrap();
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
        subscribe(&mut tx, &mut rx, 1, vec![Topic::new("files")]).await;
        std::fs::write(root.path().join("first.txt"), "one").unwrap();
        assert!(
            matches!(next(&mut rx).await, Reply::Changed(event) if event.topic.name == "files")
        );
        let primary = Topic::keyed("files", root.path().to_str().unwrap());
        let alias = Topic::keyed("files", root.path().join(".").to_str().unwrap());
        subscribe(&mut tx, &mut rx, 2, vec![primary, alias.clone()]).await;
        subscribe(&mut tx, &mut rx, 3, vec![alias.clone()]).await;
        std::fs::write(root.path().join("second.txt"), "two").unwrap();
        assert!(matches!(next(&mut rx).await, Reply::Changed(event) if event.topic == alias));
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
