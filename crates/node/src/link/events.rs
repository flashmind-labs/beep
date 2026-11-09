//! An authenticated peer can replace topics and send commands while updates arrive.

use super::Offers;
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{SessionsReply, SessionsRequest},
    events::{self, CallReply, Operation, Reply, Request, Topic},
};
use futures::{FutureExt, StreamExt, stream};
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
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_util::task::AbortOnDropHandle;

const WRITE_WAIT: Duration = Duration::from_secs(15);
const MAX_CALLS: usize = 16;
const MAX_FILE_ROOTS: usize = 512;

mod git;
mod snapshots;
pub(crate) mod sources;
use snapshots::Snapshots;
use sources::Subscriptions;

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
    let mut topics = Vec::new();
    let mut queries = Vec::new();
    let mut subscriptions = Subscriptions::new();
    let mut roots = BTreeMap::new();
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
        let snapshot_due = snapshots.due();
        tokio::select! {
            _ = async {
                match snapshot_due {
                    Some(due) => tokio::time::sleep_until(due).await,
                    None => std::future::pending().await,
                }
            }, if snapshot_due.is_some() => {},
            request = rx.recv::<Request>() => match request? {
                None => return Ok(()),
                Some(Request::Watch { id, topics: wanted, queries: wanted_queries }) => {
                    let valid = if wanted.len() > events::MAX_TOPICS {
                        Err(anyhow::anyhow!("too many subscription topics"))
                    } else if wanted.iter().any(|topic| !topic.valid()) {
                        Err(anyhow::anyhow!("invalid subscription topics"))
                    } else { Snapshots::validate(&wanted_queries) };
                    let source_topics = sources(&wanted, &wanted_queries);
                    let result = match valid {
                        Ok(()) => sources::subscribe(&offers, &source_topics).await,
                        Err(error) => Err(error),
                    };

                    match result {
                        Ok(next) => {
                            subscriptions = next;
                            roots = sources::roots(&subscriptions);
                            topics = wanted;
                            queries = wanted_queries;
                            snapshots.replace(&offers, &topics, &queries, &roots);
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
            Some((_, event)) = subscriptions.next(), if !subscriptions.is_empty() => {
                // Overlapping keys can receive the same broad invalidation.
                // Drain a bounded ready batch before reconciling its sources.
                let mut batch = vec![event];

                for _ in 0..subscriptions.len() {
                    let Some(Some((_, event))) = subscriptions.next().now_or_never() else {
                        break;
                    };
                    batch.push(event);
                }

                let mut changed = HashSet::new();
                let mut lagged = false;

                for event in batch {
                    match event {
                        Ok(event) => { changed.insert(event.topic); }
                        Err(BroadcastStreamRecvError::Lagged(_)) => lagged = true,
                    }
                }

                let reconcile = lagged || broad_files(&sources(&topics, &queries))
                    && changed.iter().any(|topic| matches!(topic.name(), "agents" | "projects" | "*"));
                if reconcile {
                    sources::reconcile(&offers, &subscriptions).await?;
                    roots = sources::roots(&subscriptions);
                }
                // Publication can add a previously unknown exact plugin topic.
                if reconcile || changed.iter().any(|topic| !snapshots.contains(topic) && offers.agents.events.current(topic).is_some()) {
                    snapshots.replace(&offers, &topics, &queries, &roots);
                }

                if lagged {
                    snapshots.reset();
                }

                for topic in changed {
                    snapshots.changed(&topic);
                }
            },
            changed = membership.changed(), if watches_files(&sources(&topics, &queries)) => {
                if changed.is_err() { return Ok(()); }
                project_changes = membership.borrow_and_update().as_ref().map(beep_mesh::Mesh::changes);
                sources::reconcile(&offers, &subscriptions).await?;
                roots = sources::roots(&subscriptions);
                snapshots.replace(&offers, &topics, &queries, &roots);
            },
            change = async { match &mut project_changes { Some(changes) => changes.recv().await, None => std::future::pending().await } }, if watches_files(&sources(&topics, &queries)) => {
                let reconcile = match change {
                    Ok(beep_mesh::Key::Project(_) | beep_mesh::Key::Sync(_)) | Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => { project_changes = None; false },
                    _ => false,
                };

                if reconcile {
                    sources::reconcile(&offers, &subscriptions).await?;
                    roots = sources::roots(&subscriptions);
                    snapshots.replace(&offers, &topics, &queries, &roots);
                }
            },
            _ = heartbeat.tick() => send(&mut tx, &Reply::Pong { id: 0 }).await?,
        }
    }
}

fn sources(topics: &[Topic], queries: &[events::Query]) -> Vec<Topic> {
    let mut sources = topics.to_vec();
    sources.extend(queries.iter().map(|query| query.topic.clone()));

    if sources.iter().any(|topic| topic.name() == "agents") {
        sources.extend([Topic::new("agent"), Topic::new("moves")]);
    }

    if sources.iter().any(|topic| topic.name() == "agent") {
        sources.push(Topic::new("agents"));
    }
    if sources
        .iter()
        .any(|topic| matches!(topic.name(), "processes" | "process-output"))
    {
        sources.extend([Topic::new("processes"), Topic::new("process-output")]);
    }

    if sources
        .iter()
        .any(|topic| matches!(topic.name(), "terminals" | "terminal-output"))
    {
        sources.extend([Topic::new("terminals"), Topic::new("terminal-output")]);
    }

    if broad_files(&sources) {
        sources.extend([Topic::new("agents"), Topic::new("projects")]);
    }

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
#[path = "../tests/link_events.rs"]
mod tests;
