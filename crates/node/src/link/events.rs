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
    collections::{BTreeMap, HashMap, HashSet},
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
/// How often a quiet stream to a desktop or phone says it is alive. Released
/// apps drop a stream after 35 s of silence, so it stays well under that.
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(15);
const MAX_CALLS: usize = 16;
const MAX_FILE_ROOTS: usize = 512;

mod git;
mod snapshots;
pub(crate) mod sources;
pub(super) mod subscriptions;
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
    let mut snapshots = Snapshots::new();
    let stream = snapshots.stream();
    tracing::info!(%stream, "subscription stream opened");
    // Several controllers may each keep one stream per demanded topic.
    static READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(256);
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
    let mut calls = JoinSet::new();
    let mut pending = HashSet::new();
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    send(&mut tx, Reply::Ready).await?;

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
            request = rx.recv::<Request>() => {
                match request? {
                None => return Ok(()),
                Some(Request::Watch { id, topics: wanted, queries: wanted_queries }) => {
                    let started = Instant::now();
                    tracing::info!(%stream, watch = id, topics = wanted.len(), queries = wanted_queries.len(), "subscription watch received");
                    for query in &wanted_queries {
                        tracing::info!(%stream, watch = id, query = query.id, topic = query.topic.name(), "subscription query received");
                    }

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
                            send(&mut tx, Reply::Watching { id }).await?;
                            tracing::info!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch acknowledged");
                        }
                        Err(error) => {
                            tracing::warn!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch rejected");
                            send(&mut tx, Reply::Error { id, reason: format!("{error:#}") }).await?;
                        },
                    }
                }
                Some(Request::Refresh { id }) => {
                    snapshots.reset();
                    send(&mut tx, Reply::Watching { id }).await?;
                },
                Some(Request::Ping { id }) => send(&mut tx, Reply::Pong { id }).await?,
                Some(Request::Call { id, operation }) => {
                    if calls.len() >= MAX_CALLS || !pending.insert(id) {
                        send(&mut tx, Reply::Error { id, reason: "too many pending calls or duplicate call id".into() }).await?;
                    } else {
                        let agents = offers.agents.clone();
                        calls.spawn(async move { (id, timeout(Duration::from_secs(120), execute(agents, caller, *operation)).await) });
                    }
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
                send(&mut tx, reply).await?;
            },
            snapshot = snapshots.next() => if let Some(snapshot) = snapshot? {
                let snapshot = Reply::from(snapshot);
                let started = Instant::now();
                let (topic, query, success) = match &snapshot {
                    Reply::Snapshot(value) => (value.topic.name().to_owned(), value.query, true),
                    Reply::SnapshotError { topic, query, .. } => (topic.name().to_owned(), *query, false),
                    _ => unreachable!("snapshot queue only returns snapshots"),
                };
                tracing::debug!(%stream, %topic, query, success, "subscription snapshot sending");
                send(&mut tx, snapshot).await?;
                tracing::debug!(%stream, %topic, query, success, elapsed_ms = started.elapsed().as_millis() as u64, "subscription snapshot sent");
            },
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

                let mut changed = HashMap::new();
                let mut lagged = false;

                for event in batch {
                    match event {
                        Ok(event) => {
                            let kind = changed.entry(event.topic).or_insert(event.kind);
                            *kind = (*kind).max(event.kind);
                        }
                        Err(BroadcastStreamRecvError::Lagged(_)) => lagged = true,
                    }
                }

                let reconcile = lagged || broad_files(&sources(&topics, &queries))
                    && changed.keys().any(|topic| matches!(topic.name(), "agents" | "projects" | "*"));
                if reconcile {
                    sources::reconcile(&offers, &subscriptions).await?;
                    roots = sources::roots(&subscriptions);
                }
                // Publication can add a previously unknown exact plugin topic.
                if reconcile || changed.keys().any(|topic| !snapshots.contains(topic) && offers.agents.events.current(topic).is_some()) {
                    snapshots.replace(&offers, &topics, &queries, &roots);
                }

                if lagged {
                    snapshots.reset();
                }

                // A root bound without Git metadata may have become a repository.
                for subscription in subscriptions.values() {
                    if subscription.source.retry_git(changed.keys()) {
                        subscription.source.reconcile(&offers).await?;
                    }
                }

                for (topic, kind) in changed {
                    snapshots.changed(&topic, kind);
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
            _ = heartbeat.tick() => send(&mut tx, Reply::Pong { id: 0 }).await?,
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

async fn send(tx: &mut LinkTx, reply: Reply) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(&reply))
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

    let same_roots = *crate::lock(roots) == wanted;
    let held = watcher
        .as_ref()
        .map(|watcher| crate::lock(&watcher.bindings).clone())
        .unwrap_or_default();

    if same_roots && held.values().all(|dependencies| has_git(dependencies)) {
        return Ok(());
    }

    if wanted.is_empty() {
        *watcher = None;
    } else {
        // A root kept from the last pass keeps its Git paths without asking Git again.
        // One without them asks again: it may have become a repository, or Git failed.
        let bindings = stream::iter(wanted.clone())
            .map(|(topic, root)| {
                let kept = held
                    .get(&topic)
                    .filter(|dependencies| {
                        dependencies.first() == Some(&root) && has_git(dependencies)
                    })
                    .cloned();
                async move {
                    if let Some(dependencies) = kept {
                        return (topic, dependencies);
                    }

                    let mut dependencies = vec![root.clone()];

                    match git::watch_paths(&root).await {
                        Ok(paths) => dependencies.extend(paths),
                        Err(error) => {
                            log::debug!("Git metadata notifications unavailable: {error:#}")
                        }
                    }
                    (topic, dependencies)
                }
            })
            .buffer_unordered(8)
            .collect::<FileBindings>()
            .await;

        if same_roots && bindings == held {
            return Ok(());
        }

        if watcher.is_none() {
            let bindings = Arc::new(Mutex::new(BTreeMap::<Topic, Vec<PathBuf>>::new()));
            let changed_roots = bindings.clone();
            let hub = offers.agents.events.clone();
            let paths = crate::events::paths::Paths::new(move |paths| {
                let held = crate::lock(&changed_roots);

                for (topic, dependencies) in held.iter() {
                    // The root comes first; the rest are its Git metadata.
                    let Some((root, git)) = dependencies.split_first() else {
                        continue;
                    };

                    if git.iter().any(|dependency| paths.contains(dependency)) {
                        hub.changed(topic.clone());
                    } else if paths.contains(root) {
                        hub.changed_work_tree(topic.clone());
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

/// Whether a root's bindings include its Git metadata after the root itself.
fn has_git(dependencies: &[PathBuf]) -> bool {
    dependencies.len() > 1
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

pub(super) async fn execute(
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
