//! An authenticated peer can replace topics and send commands while updates arrive.

use super::Offers;
use crate::events::{HEARTBEAT, Touched};
use anyhow::{Context, Result};
use beep_proto::node::{
    LinkRx, LinkTx,
    events::{self, Incoming, Request, Topic},
    subscriptions::{self, Reply},
};
use beep_proto::watched;
use futures::{FutureExt, StreamExt};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    time::Duration,
};
use tokio::{
    sync::broadcast::error::RecvError,
    task::JoinSet,
    time::{Instant, MissedTickBehavior, interval_at, timeout},
};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

const WRITE_WAIT: Duration = Duration::from_secs(15);
const MAX_CALLS: usize = 16;
const MAX_QUEUED_WATCHES: usize = 16;

mod ask;
mod git;
mod git_reads;
mod roots;
mod snapshots;
pub(crate) mod sources;
pub(super) use ask::execute;
use roots::broad_files;
use snapshots::Snapshots;
use sources::{Pass, Passes, Subscriptions};

/// Which stream protocol a loop serves. `events/1` and `peer-subscriptions/1` carry the same
/// topics and snapshots, with different bytes, and only `events/1` takes calls.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wire {
    Events,
    Subscriptions,
}

/// What a caller of a subscription stream may watch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reach {
    /// Any topic: a controller.
    Whole,
    /// Only what another node of the mesh already reads with a one-off ask:
    /// the `agents` list and how code hosts are reached.
    Node,
}

impl Reach {
    fn allows(self, topics: &[Topic], queries: &[events::Query]) -> bool {
        self == Self::Whole
            || queries.is_empty()
                && topics.iter().all(|topic| {
                    topic.resource_segments().is_empty()
                        && matches!(topic.name(), "agents" | events::FORGE_REACH_TOPIC)
                })
    }
}

/// A watch being looked up: its sources are found while the stream goes on.
struct Watched {
    id: u64,
    started: Instant,
    topics: Vec<Topic>,
    queries: Vec<events::Query>,
    deltas: bool,
    query_deltas: bool,
    result: Result<Subscriptions>,
}

/// A watch read from the stream and not looked up yet.
struct Queued {
    id: u64,
    topics: Vec<Topic>,
    queries: Vec<events::Query>,
    deltas: bool,
    query_deltas: bool,
}

/// Checks a queued watch and starts looking its sources up, or answers it with an error.
async fn begin_watch(
    tx: &mut LinkTx,
    wire: Wire,
    reach: Reach,
    offers: &Offers,
    stream: &uuid::Uuid,
    queued: Queued,
    watching: &mut JoinSet<Watched>,
) -> Result<()> {
    let Queued {
        id,
        topics: wanted,
        queries: wanted_queries,
        deltas,
        query_deltas,
    } = queued;
    let started = Instant::now();
    tracing::info!(%stream, watch = id, topics = wanted.len(), queries = wanted_queries.len(), "subscription watch received");
    for query in &wanted_queries {
        tracing::info!(%stream, watch = id, query = query.id, topic = query.topic.name(), "subscription query received");
    }

    let valid = if !reach.allows(&wanted, &wanted_queries) {
        Err(anyhow::anyhow!(
            "a node may watch only the agents and forge-reach topics"
        ))
    } else if wanted.len() > events::MAX_TOPICS {
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
                Watched {
                    id,
                    started,
                    topics: wanted,
                    queries: wanted_queries,
                    deltas,
                    query_deltas,
                    result,
                }
            });
        }
        Err(error) => {
            tracing::warn!(%stream, watch = id, elapsed_ms = started.elapsed().as_millis() as u64, "subscription watch rejected");
            send(
                tx,
                wire,
                Reply::Error {
                    id,
                    reason: format!("{error:#}"),
                },
            )
            .await?;
        }
    }

    Ok(())
}

pub(crate) async fn serve(
    tx: LinkTx,
    rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
    local: bool,
) -> Result<()> {
    let opening = Opening {
        wire: Wire::Events,
        local,
        reach: Reach::Whole,
        compress: false,
    };

    run(opening, tx, rx, offers, caller).await
}

pub(super) async fn serve_subscriptions(
    tx: LinkTx,
    rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
    local: bool,
    reach: Reach,
    compress: bool,
) -> Result<()> {
    let opening = Opening {
        wire: Wire::Subscriptions,
        local,
        reach,
        compress,
    };

    run(opening, tx, rx, offers, caller).await
}

/// How a stream was opened: its protocol, whether the caller is the person
/// at this machine (not held to the lending setting), and what it may see.
struct Opening {
    wire: Wire,
    local: bool,
    reach: Reach,
    compress: bool,
}

async fn run(
    opening: Opening,
    mut tx: LinkTx,
    mut rx: LinkRx,
    offers: Offers,
    caller: iroh::EndpointId,
) -> Result<()> {
    let Opening {
        wire,
        local,
        reach,
        compress,
    } = opening;
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
    if compress && wire == Wire::Subscriptions {
        send(&mut tx, wire, Reply::ReadyCompressed).await?;
        tx.enable_zstd();
        rx.enable_zstd();
    } else {
        send(&mut tx, wire, Reply::Ready).await?;
    }

    loop {
        while watching.is_empty()
            && let Some(next) = queued.pop_front()
        {
            begin_watch(&mut tx, wire, reach, &offers, &stream, next, &mut watching).await?;
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
                anyhow::ensure!(local || offers.lends_to_peers(), "this machine is no longer shared");

                let (request, deltas, query_deltas) = match request? {
                    None => (None, false, false),
                    Some((Incoming::Request(request), deltas, query_deltas)) => {
                        (Some(request), deltas, query_deltas)
                    }
                    // From a newer app: refuse the call, keep the stream.
                    Some((Incoming::Unread { id, reason }, _, _)) => {
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
                        queued.push_back(Queued {
                            id,
                            topics,
                            queries,
                            deltas,
                            query_deltas,
                        });
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
                let Watched {
                    id,
                    started,
                    topics: wanted,
                    queries: wanted_queries,
                    deltas,
                    query_deltas,
                    result,
                } = done.context("a topic watch stopped")?;

                match result {
                    Ok(next) => {
                        subscriptions = next;
                        roots = sources::roots(&subscriptions);
                        topics = wanted;
                        queries = wanted_queries;
                        snapshots.follow_deltas(deltas);
                        snapshots.follow_query_deltas(query_deltas);
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
                            let (kind, touched) = changed
                                .entry(event.topic)
                                .or_insert((event.kind, Touched::Nothing));
                            *kind = (*kind).max(event.kind);
                            touched.add(event.touched);
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

                for (topic, (kind, touched)) in changed {
                    snapshots.changed(&topic, kind, &touched);
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
                        snapshots.changed(topic, crate::events::ChangeKind::Any, &Touched::All);
                    }
                }
            },
            _ = heartbeat.tick() => send(&mut tx, wire, Reply::Pong { id: 0 }).await?,
        }
    }
}

/// The next request of the stream, as `events/1` names it, and whether its
/// sender takes the `agents` list and external queries as deltas. `events/1`
/// has no such flags.
async fn read_request(wire: Wire, rx: &mut LinkRx) -> Result<Option<(Incoming, bool, bool)>> {
    match wire {
        Wire::Events => Ok(rx
            .recv_with(Incoming::read)
            .await?
            .map(|read| (read, false, false))),
        Wire::Subscriptions => {
            Ok(rx
                .recv::<subscriptions::Request>()
                .await?
                .map(|request| match request {
                    subscriptions::Request::Watch {
                        id,
                        topic,
                        queries,
                        deltas,
                        query_deltas,
                    } => (
                        Incoming::Request(Request::Watch {
                            id,
                            topics: vec![topic],
                            queries,
                        }),
                        deltas,
                        query_deltas,
                    ),
                    subscriptions::Request::Refresh { id } => {
                        (Incoming::Request(Request::Refresh { id }), false, false)
                    }
                    subscriptions::Request::Ping { id } => {
                        (Incoming::Request(Request::Ping { id }), false, false)
                    }
                }))
        }
    }
}

fn sources(topics: &[Topic], queries: &[events::Query]) -> Vec<Topic> {
    let mut sources = topics.to_vec();
    sources.extend(queries.iter().map(|query| query.topic.clone()));

    if sources.iter().any(|topic| topic.name() == "agents") {
        sources.extend([
            Topic::new("agent"),
            Topic::new("agent-log"),
            Topic::new("moves"),
        ]);
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

#[cfg(test)]
#[path = "../tests/link_events.rs"]
mod tests;
