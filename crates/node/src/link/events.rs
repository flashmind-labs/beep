//! An authenticated peer can replace topics and send commands while updates arrive.

use super::Offers;
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{SessionsReply, SessionsRequest},
    events::{self, CallReply, Incoming, Operation, Request, Topic},
    subscriptions::{self, Reply},
};
use beep_proto::watched;
use futures::{FutureExt, StreamExt, future::join_all};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
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
const MAX_QUEUED_WATCHES: usize = 16;
const MAX_FILE_ROOTS: usize = 512;
const TOO_MANY_ROOTS: &str = "too many watched directories; subscribe to specific file roots";

mod git;
mod git_reads;
mod snapshots;
pub(crate) mod sources;
use snapshots::Snapshots;
use sources::{Pass, Passes, Subscriptions};

type FileBindings = BTreeMap<Topic, Vec<PathBuf>>;

struct FileWatcher {
    paths: crate::events::paths::Paths,
    bindings: Arc<Mutex<FileBindings>>,
}

/// Which stream protocol a loop serves. `events/1` and `peer-subscriptions/1` carry the same
/// topics and snapshots, with different bytes, and only `events/1` takes calls.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wire {
    Events,
    Subscriptions,
}

/// A watch being looked up: its sources are found while the stream goes on.
struct Watched {
    id: u64,
    started: Instant,
    topics: Vec<Topic>,
    queries: Vec<events::Query>,
    deltas: bool,
    result: Result<Subscriptions>,
}

/// A watch read from the stream and not looked up yet.
struct Queued {
    id: u64,
    topics: Vec<Topic>,
    queries: Vec<events::Query>,
    deltas: bool,
}

/// Checks a queued watch and starts looking its sources up, or answers it with an error.
async fn begin_watch(
    tx: &mut LinkTx,
    wire: Wire,
    offers: &Offers,
    stream: &uuid::Uuid,
    queued: Queued,
    watching: &mut JoinSet<Watched>,
) -> Result<()> {
    let Queued { id, topics: wanted, queries: wanted_queries, deltas } = queued;
    let started = Instant::now();
    tracing::info!(%stream, watch = id, topics = wanted.len(), queries = wanted_queries.len(), "subscription watch received");
    for query in &wanted_queries {
        tracing::info!(%stream, watch = id, query = query.id, topic = query.topic.name(), "subscription query received");
    }

    let valid = if wanted.len() > events::MAX_TOPICS {
        Err(anyhow::anyhow!("too many subscription topics"))
    } else if wanted.iter().any(|topic| !topic.valid()) {
        Err(anyhow::anyhow!("invalid subscription topics"))
    } else {
        Snapshots::validate(&wanted_queries)
    };

    match valid {
        Ok(()) => {
            let offers = offers.clone();
            let source_topics = sources(&wanted, &wanted_queries);
            watching.spawn(async move {
                let result = sources::subscribe(&offers, &source_topics).await;
                Watched { id, started, topics: wanted, queries: wanted_queries, deltas, result }
            });
        }
        Err(error) => {
            tracing::warn!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch rejected");
            send(tx, wire, Reply::Error { id, reason: format!("{error:#}") }).await?;
        }
    }

    Ok(())
}

pub(crate) async fn serve(
    tx: LinkTx,
    rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    run(Wire::Events, tx, rx, offers, caller).await
}

pub(super) async fn serve_subscriptions(
    tx: LinkTx,
    rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    run(Wire::Subscriptions, tx, rx, offers, caller).await
}

async fn run(
    wire: Wire,
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    let mut snapshots = Snapshots::new();
    let stream = snapshots.stream();
    tracing::info!(%stream, "subscription stream opened");
    // Several controllers may each keep one stream per demanded topic; each
    // protocol has its own limit.
    static EVENT_READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(256);
    static PEER_READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(256);
    let readers = match wire {
        Wire::Events => &EVENT_READERS,
        Wire::Subscriptions => &PEER_READERS,
    };
    let _reader = readers.try_acquire().context("too many topic readers")?;
    let mut membership = offers.mesh.subscribe();
    let mut project_changes = watched::read_and_update(&mut membership, |mesh| {
        mesh.as_ref().map(beep_mesh::Mesh::changes)
    });
    offers.agents.start_event_bridge();
    let mut topics = Vec::new();
    let mut queries = Vec::new();
    let mut subscriptions = Subscriptions::new();
    let mut roots = BTreeMap::new();
    let mut passes = Passes::new(&offers);
    // A watch was answered before its watchers bound: read once more after.
    let mut unbound = false;
    // At most one watch is looked up at a time; the others wait here and
    // every other request is read meanwhile.
    let mut watching = JoinSet::new();
    let mut queued = VecDeque::new();
    let mut calls = JoinSet::new();
    let mut pending = HashSet::new();
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    send(&mut tx, wire, Reply::Ready).await?;

    loop {
        while watching.is_empty()
            && let Some(next) = queued.pop_front()
        {
            begin_watch(&mut tx, wire, &offers, &stream, next, &mut watching).await?;
        }

        snapshots.start(&offers, caller);
        let snapshot_due = snapshots.due();
        tokio::select! {
            _ = async {
                match snapshot_due {
                    Some(due) => tokio::time::sleep_until(due).await,
                    None => std::future::pending().await,
                }
            }, if snapshot_due.is_some() => {},
            request = read_request(wire, &mut rx) => {
                if wire == Wire::Subscriptions {
                    anyhow::ensure!(offers.lends_to_peers() || offers.mesh.get().is_some_and(|mesh| mesh.me() == caller), "this machine is no longer shared");
                }

                let (request, deltas) = match request? {
                    None => (None, false),
                    Some((Incoming::Request(request), deltas)) => (Some(request), deltas),
                    // From a newer app: refuse the call, keep the stream.
                    Some((Incoming::Unread { id, reason }, _)) => {
                        tracing::warn!(%stream, call = id, %reason, "subscription call not read");
                        send(&mut tx, wire, Reply::Error { id, reason }).await?;
                        continue;
                    }
                };

                match request {
                None => return Ok(()),
                Some(Request::Watch { id, topics, queries }) => {
                    // Watches are looked up one at a time, in the order sent, so a later
                    // one is never answered or applied before an earlier one.
                    if queued.len() >= MAX_QUEUED_WATCHES {
                        send(&mut tx, wire, Reply::Error { id, reason: "too many pending watches".into() }).await?;
                    } else {
                        queued.push_back(Queued { id, topics, queries, deltas });
                    }
                }
                Some(Request::Refresh { id }) => {
                    snapshots.reset();
                    send(&mut tx, wire, Reply::Watching { id }).await?;
                },
                Some(Request::Ping { id }) => send(&mut tx, wire, Reply::Pong { id }).await?,
                Some(Request::Call { id, operation }) => {
                    if calls.len() >= MAX_CALLS || !pending.insert(id) {
                        send(&mut tx, wire, Reply::Error { id, reason: "too many pending calls or duplicate call id".into() }).await?;
                    } else {
                        let agents = offers.agents.clone();
                        calls.spawn(async move { (id, timeout(Duration::from_secs(120), execute(agents, caller, *operation)).await) });
                    }
                }
                }
            },
            Some(done) = watching.join_next(), if !watching.is_empty() => {
                let Watched { id, started, topics: wanted, queries: wanted_queries, deltas, result } = done.context("a topic watch stopped")?;

                match result {
                    Ok(next) => {
                        subscriptions = next;
                        roots = sources::roots(&subscriptions);
                        topics = wanted;
                        queries = wanted_queries;
                        snapshots.follow_deltas(deltas);
                        snapshots.replace(&offers, &topics, &queries, &roots);
                        // The first snapshots do not wait for the watchers.
                        passes.start(Pass::bind(&subscriptions));
                        unbound = true;
                        send(&mut tx, wire, Reply::Watching { id }).await?;
                        tracing::info!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch acknowledged");
                    }
                    Err(error) => {
                        tracing::warn!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch rejected");
                        send(&mut tx, wire, Reply::Error { id, reason: format!("{error:#}") }).await?;
                    }
                }
            },
            Some(result) = calls.join_next(), if !calls.is_empty() => {
                let (id, result) = result.context("a node command stopped")?;
                pending.remove(&id);

                match result {
                    Ok(Ok(reply)) => write(&mut tx, &events::Reply::Result { id, reply: Box::new(reply) }).await?,
                    Ok(Err(error)) => send(&mut tx, wire, Reply::Error { id, reason: format!("{error:#}") }).await?,
                    Err(_) => send(&mut tx, wire, Reply::Error { id, reason: "command timed out; its outcome may be unknown".into() }).await?,
                }
            },
            snapshot = snapshots.next() => if let Some(snapshot) = snapshot? {
                let started = Instant::now();
                let (topic, query, success) = match &snapshot {
                    Reply::Snapshot(value) => (value.topic.name().to_owned(), value.query, true),
                    Reply::SnapshotError { topic, query, .. } => (topic.name().to_owned(), *query, false),
                    _ => unreachable!("snapshot queue only returns snapshots"),
                };
                tracing::debug!(%stream, %topic, query, success, "subscription snapshot sending");
                send(&mut tx, wire, snapshot).await?;
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
                    passes.start(Pass::all(&subscriptions));
                }
                // Publication can add a previously unknown exact plugin topic.
                if changed.keys().any(|topic| !snapshots.contains(topic) && offers.agents.events.current(topic).is_some()) {
                    snapshots.replace(&offers, &topics, &queries, &roots);
                }

                if lagged {
                    snapshots.reset();
                }

                if let Some(pass) = Pass::git(&subscriptions, changed.keys()) {
                    passes.start(pass);
                }

                for (topic, kind) in changed {
                    snapshots.changed(&topic, kind);
                }
            },
            changed = membership.changed(), if watches_files(&sources(&topics, &queries)) => {
                if changed.is_err() { return Ok(()); }
                project_changes = watched::read_and_update(&mut membership, |mesh| mesh.as_ref().map(beep_mesh::Mesh::changes));
                passes.start(Pass::all(&subscriptions));
            },
            change = async { match &mut project_changes { Some(changes) => changes.recv().await, None => std::future::pending().await } }, if watches_files(&sources(&topics, &queries)) => {
                let reconcile = match change {
                    Ok(beep_mesh::Key::Project(_) | beep_mesh::Key::Sync(_)) | Err(RecvError::Lagged(_)) => true,
                    Err(RecvError::Closed) => { project_changes = None; false },
                    _ => false,
                };

                if reconcile {
                    passes.start(Pass::all(&subscriptions));
                }
            },
            result = passes.finished(), if !passes.is_empty() => {
                // The roots are published now, and the watchers announce what changed.
                // A pass that failed leaves its own watches unbound, not the stream.
                if let Err(error) = result {
                    tracing::warn!(%stream, "a topic pass failed: {error:#}");
                }

                roots = sources::roots(&subscriptions);
                snapshots.replace(&offers, &topics, &queries, &roots);

                // A change made between the first read and the binding is
                // not announced by a watcher that was not there yet.
                if unbound && passes.is_empty() {
                    unbound = false;

                    for topic in roots.keys() {
                        snapshots.changed(topic, crate::events::ChangeKind::Any);
                    }
                }
            },
            _ = heartbeat.tick() => send(&mut tx, wire, Reply::Pong { id: 0 }).await?,
        }
    }
}

/// The next request of the stream, as `events/1` names it, and whether its
/// sender takes the `agents` list as deltas. `events/1` has no such flag.
async fn read_request(wire: Wire, rx: &mut LinkRx) -> Result<Option<(Incoming, bool)>> {
    match wire {
        Wire::Events => Ok(rx.recv_with(Incoming::read).await?.map(|read| (read, false))),
        Wire::Subscriptions => Ok(rx
            .recv::<subscriptions::Request>()
            .await?
            .map(|request| match request {
                subscriptions::Request::Watch { id, topic, queries, deltas } => (
                    Incoming::Request(Request::Watch { id, topics: vec![topic], queries }),
                    deltas,
                ),
                subscriptions::Request::Refresh { id } => {
                    (Incoming::Request(Request::Refresh { id }), false)
                }
                subscriptions::Request::Ping { id } => {
                    (Incoming::Request(Request::Ping { id }), false)
                }
            })),
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

/// Sends `reply` as `wire` words it.
async fn send(tx: &mut LinkTx, wire: Wire, reply: Reply) -> Result<()> {
    match wire {
        Wire::Events => write(tx, &events::Reply::from(reply)).await,
        Wire::Subscriptions => write(tx, &reply).await,
    }
}

async fn write(tx: &mut LinkTx, reply: &impl serde::Serialize) -> Result<()> {
    timeout(WRITE_WAIT, tx.send(reply))
        .await
        .context("subscription reader stopped receiving")?
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

/// The roots `topics` name now, by topic.
async fn resolve(offers: &Offers, topics: &[Topic]) -> Result<BTreeMap<Topic, PathBuf>> {
    let mut wanted = BTreeMap::new();

    if broad_files(topics) {
        for root in offers.agents.notification_roots().await? {
            let key = root.to_str().context("a notification root is not UTF-8")?;
            wanted.insert(Topic::keyed("files", key), root);
        }
    }

    // The roots are checked together: each check can wait on a worktree list.
    let projects: Vec<_> = project_files(offers)
        .into_iter()
        .filter(|(topic, _)| topics.iter().any(|wanted| wanted.covers(topic)))
        .collect();
    let reached = join_all(
        projects
            .iter()
            .map(|(_, path)| offers.agents.reachable(path, crate::agents::reach::Access::Read)),
    )
    .await;

    for ((topic, _), reached) in projects.into_iter().zip(reached) {
        match reached {
            Ok(root) => {
                wanted.insert(topic, root);
            }
            Err(error) if topics.iter().any(|wanted| wanted == &topic) => return Err(error),
            Err(_) => {}
        }
    }

    let literal: Vec<_> = topics
        .iter()
        .filter(|topic| topic.name() == "files" && !topic.is_pattern())
        .filter_map(|topic| match topic.resource_segments() {
            // Local paths are opaque literal segments. Project aliases use their mesh names.
            [path] if std::path::Path::new(path).is_absolute() || path.starts_with("~/") => {
                Some((topic, path))
            }
            _ => None,
        })
        .collect();
    let reached = join_all(
        literal
            .iter()
            .map(|(_, path)| offers.agents.reachable(path, crate::agents::reach::Access::Read)),
    )
    .await;

    for ((topic, _), root) in literal.into_iter().zip(reached) {
        wanted.insert(topic.clone(), root?);
    }

    if wanted.len() > MAX_FILE_ROOTS {
        bail!(TOO_MANY_ROOTS);
    }

    Ok(wanted)
}

/// Whether a root's bindings include its Git metadata after the root itself.
fn has_git(dependencies: &[PathBuf]) -> bool {
    dependencies.len() > 1
}

/// Resolve public project aliases through this node's signed mesh placement records.
fn project_files(offers: &Offers) -> BTreeMap<Topic, String> {
    use beep_mesh::Key;
    use beep_proto::node::mesh_state::{StateValue, SyncSpec};
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

    for (name, _, place) in super::from_mesh::placed_projects(&mesh, mesh.me()) {
        let topic = Topic::scoped("files", name.split('/'));
        if topic.valid() {
            projects.insert(topic, place.folder);
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
            let reply = ask(&request, |tx, rx| {
                crate::agents::serve_sessions(tx, rx, agents, caller, None)
            })
            .await?
            .context("session request closed without a reply")?;
            Ok(CallReply::Sessions(Box::new(reply)))
        }
    }
}

/// Sends one `request` to `serve` on an in-memory link and reads its first
/// reply; `None` when the stream ends without one.
async fn ask<Req, Rep, F>(request: &Req, serve: impl FnOnce(LinkTx, LinkRx) -> F) -> Result<Option<Rep>>
where
    Req: serde::Serialize,
    Rep: serde::de::DeserializeOwned,
    F: Future<Output = Result<()>> + Send + 'static,
{
    let link = beep_proto::node::local_link();
    let _task = AbortOnDropHandle::new(tokio::spawn(serve(link.node.0, link.node.1)));
    let (mut tx, mut rx) = link.caller;
    tx.send(request).await?;
    rx.recv().await
}

#[cfg(test)]
#[path = "../tests/link_events.rs"]
mod tests;
