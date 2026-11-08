//! An authenticated peer can replace topics and send commands while updates arrive.

use super::Offers;
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{SessionsReply, SessionsRequest},
    events::{self, CallReply, Operation, Reply, Request, Topic},
};
use futures::{StreamExt, stream};
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

mod git;
mod snapshots;
use snapshots::Snapshots;

type FileBindings = BTreeMap<Topic, Vec<PathBuf>>;

struct FileWatcher {
    paths: crate::events::paths::Paths,
    bindings: Arc<Mutex<FileBindings>>,
}

pub(crate) async fn serve(
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    static READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(32);
    let _reader = READERS.try_acquire().context("too many topic readers")?;
    let mut membership = offers.mesh.subscribe();
    let mut project_changes = membership
        .borrow_and_update()
        .as_ref()
        .map(beep_mesh::Mesh::changes);
    offers.agents.start_event_bridge();
    let mut changes = offers.agents.events.subscribe();
    let mut topics = Vec::new();
    let mut queries = Vec::new();
    let roots = Arc::new(Mutex::new(BTreeMap::<Topic, PathBuf>::new()));
    let mut watcher = None;
    let mut _source_lease = None;
    let mut snapshots = Snapshots::new();
    let mut calls = JoinSet::new();
    let mut pending = HashSet::new();
    let mut heartbeat = interval_at(
        Instant::now() + Duration::from_secs(10),
        Duration::from_secs(10),
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    send(&mut tx, &Reply::Ready).await?;
    loop {
        snapshots.start(&offers, caller);
        tokio::select! {
            request = rx.recv::<Request>() => match request? {
                None => return Ok(()),
                Some(Request::Watch { id, topics: wanted, queries: wanted_queries }) => {
                    let valid = if wanted.len() > events::MAX_TOPICS {
                        Err(anyhow::anyhow!("too many subscription topics"))
                    } else { Snapshots::validate(&wanted_queries) };
                    let source_topics = sources(&wanted, &wanted_queries);
                    let result = match valid {
                        Ok(()) => subscribe(&offers, &mut watcher, &roots, &source_topics).await,
                        Err(error) => Err(error),
                    };
                    match result {
                        Ok(lease) => {
                            _source_lease = lease;
                            topics = wanted;
                            queries = wanted_queries;
                            snapshots.replace(&offers, &topics, &queries, &crate::lock(&roots));
                            send(&mut tx, &Reply::Watching { id }).await?;
                        }
                        Err(error) => send(&mut tx, &Reply::Error { id, reason: format!("{error:#}") }).await?,
                    }
                }
                Some(Request::Refresh { id }) => {
                    snapshots.reset();
                    send(&mut tx, &Reply::Watching { id }).await?;
                },
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
            snapshot = snapshots.next() => if let Some(snapshot) = snapshot? { send(&mut tx, &snapshot).await?; },
            event = changes.recv() => {
                match event {
                    Ok(event) => {
                        if broad_files(&sources(&topics, &queries)) && matches!(event.topic.name(), "agents" | "projects" | "*") {
                            watch(&offers, &mut watcher, &roots, &sources(&topics, &queries)).await?;
                            snapshots.replace(&offers, &topics, &queries, &crate::lock(&roots));
                        }
                        // Publication can add a previously unknown exact plugin topic.
                        if !snapshots.contains(&event.topic) && offers.agents.events.current(&event.topic).is_some() {
                            snapshots.replace(&offers, &topics, &queries, &crate::lock(&roots));
                        }
                        snapshots.changed(&event.topic);
                    }
                    Err(RecvError::Lagged(_)) => {
                        watch(&offers, &mut watcher, &roots, &sources(&topics, &queries)).await?;
                        snapshots.replace(&offers, &topics, &queries, &crate::lock(&roots));
                        snapshots.reset();
                    }
                    Err(RecvError::Closed) => return Ok(()),
                }
            },
            changed = membership.changed(), if watches_files(&sources(&topics, &queries)) => {
                if changed.is_err() { return Ok(()); }
                project_changes = membership.borrow_and_update().as_ref().map(beep_mesh::Mesh::changes);
                watch(&offers, &mut watcher, &roots, &sources(&topics, &queries)).await?;
                snapshots.replace(&offers, &topics, &queries, &crate::lock(&roots));
            },
            change = async { match &mut project_changes { Some(changes) => changes.recv().await, None => std::future::pending().await } }, if watches_files(&sources(&topics, &queries)) => {
                let reconcile = match change {
                    Ok(beep_mesh::Key::Project(_) | beep_mesh::Key::Sync(_)) | Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => { project_changes = None; false },
                    _ => false,
                };
                if reconcile {
                    watch(&offers, &mut watcher, &roots, &sources(&topics, &queries)).await?;
                    snapshots.replace(&offers, &topics, &queries, &crate::lock(&roots));
                }
            },
            _ = heartbeat.tick() => send(&mut tx, &Reply::Pong { id: 0 }).await?,
        }
    }
}

fn sources(topics: &[Topic], queries: &[events::Query]) -> Vec<Topic> {
    let mut sources = topics.to_vec();
    sources.extend(queries.iter().map(|query| query.topic.clone()));
    sources.sort();
    sources.dedup();
    sources
}

async fn send(tx: &mut LinkTx, reply: &Reply) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(reply))
        .await
        .context("event reader stopped receiving")?
}

fn watches_files(topics: &[Topic]) -> bool {
    topics
        .iter()
        .any(|topic| matches!(topic.name(), "files" | "*"))
}

fn broad_files(topics: &[Topic]) -> bool {
    topics
        .iter()
        .any(|topic| topic.name() == "*" || (topic.name() == "files" && topic.key().is_none()))
}

async fn subscribe(
    offers: &Offers,
    watcher: &mut Option<FileWatcher>,
    roots: &Arc<Mutex<BTreeMap<Topic, PathBuf>>>,
    topics: &[Topic],
) -> Result<Option<Arc<crate::agents::follow::Sources>>> {
    if topics.len() > events::MAX_TOPICS + events::MAX_QUERIES
        || topics.iter().any(|topic| !topic.valid())
    {
        bail!("invalid subscription topics");
    }
    let lease = if broad_files(topics)
        || topics
            .iter()
            .any(|topic| matches!(topic.name(), "agent" | "agents" | "*"))
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
    watcher: &mut Option<FileWatcher>,
    roots: &Arc<Mutex<BTreeMap<Topic, PathBuf>>>,
    topics: &[Topic],
) -> Result<()> {
    let mut wanted = BTreeMap::new();
    if broad_files(topics) {
        for root in offers.agents.notification_roots().await? {
            let key = root.to_str().context("a notification root is not UTF-8")?;
            wanted.insert(Topic::keyed("files", key), root);
        }
    }
    let projects = project_files(offers);
    for (topic, path) in projects {
        if topics.iter().any(|wanted| wanted.covers(&topic)) {
            match offers
                .agents
                .reachable(&path, crate::agents::reach::Access::Read)
                .await
            {
                Ok(root) => {
                    wanted.insert(topic, root);
                }
                Err(error) if topics.iter().any(|wanted| wanted == &topic) => return Err(error),
                Err(_) => {}
            }
        }
    }
    for topic in topics
        .iter()
        .filter(|topic| topic.name() == "files" && !topic.is_pattern())
    {
        if let [path] = topic.resource_segments() {
            // Local paths are opaque literal segments. Project aliases use their mesh names.
            if std::path::Path::new(path).is_absolute() || path.starts_with("~/") {
                let root = offers
                    .agents
                    .reachable(path, crate::agents::reach::Access::Read)
                    .await?;
                wanted.insert(topic.clone(), root);
            }
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
        let bindings = stream::iter(wanted.clone())
            .map(|(topic, root)| async move {
                let mut dependencies = vec![root.clone()];
                match git::watch_paths(&root).await {
                    Ok(paths) => dependencies.extend(paths),
                    Err(error) => log::debug!("Git metadata notifications unavailable: {error:#}"),
                }
                (topic, dependencies)
            })
            .buffer_unordered(8)
            .collect::<FileBindings>()
            .await;
        if watcher.is_none() {
            let bindings = Arc::new(Mutex::new(BTreeMap::<Topic, Vec<PathBuf>>::new()));
            let changed_roots = bindings.clone();
            let hub = offers.agents.events.clone();
            let paths = crate::events::paths::Paths::new(move |paths| {
                let held = crate::lock(&changed_roots);
                for (topic, dependencies) in held.iter() {
                    if dependencies
                        .iter()
                        .any(|root| paths.iter().any(|path| path == root))
                    {
                        hub.changed(topic.clone());
                    }
                }
            })?;
            *watcher = Some(FileWatcher { paths, bindings });
        }
        if let Some(watcher) = watcher.as_ref() {
            let paths = bindings.values().flatten().cloned().collect();
            watcher.paths.replace(paths).await?;
            *crate::lock(&watcher.bindings) = bindings;
        }
    }
    // A root can appear or disappear while a broad subscription remains active.
    // Reconcile it after installing the new key mapping, including any watch gap.
    let previous = std::mem::replace(&mut *crate::lock(roots), wanted.clone());
    for topic in previous.keys().chain(wanted.keys()).collect::<HashSet<_>>() {
        offers.agents.events.changed(topic.clone());
    }
    Ok(())
}

/// Resolve public project aliases through this node's signed mesh placement records.
fn project_files(offers: &Offers) -> BTreeMap<Topic, String> {
    use beep_mesh::Key;
    use beep_proto::node::mesh_state::{Place, ProjectRecord, StateValue, SyncSpec};
    let Some(mesh) = offers.mesh.get() else {
        return BTreeMap::new();
    };
    let mut projects = BTreeMap::new();
    for (key, bytes) in mesh.scan("sync/") {
        if let Key::Sync(name) = key
            && let Ok(spec) = SyncSpec::from_bytes(&bytes)
            && let Some(folder) = spec.folders.get(&mesh.me())
        {
            let topic = Topic::scoped("files", name.split('/'));
            if topic.valid() {
                projects.insert(topic, folder.clone());
            }
        }
    }
    for (key, bytes) in mesh.scan("project/") {
        if let Key::Project(name) = key
            && ProjectRecord::from_bytes(&bytes).is_ok()
            && let Some(bytes) = mesh.get(&Key::Project(format!("{name}/{}", mesh.me())))
            && let Ok(place) = Place::from_bytes(&bytes)
        {
            let topic = Topic::scoped("files", name.split('/'));
            if topic.valid() {
                projects.insert(topic, place.folder);
            }
        }
    }
    projects
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

    async fn watch(
        tx: &mut LinkTx,
        rx: &mut LinkRx,
        id: u64,
        topics: Vec<Topic>,
        queries: Vec<Query>,
    ) {
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
}
