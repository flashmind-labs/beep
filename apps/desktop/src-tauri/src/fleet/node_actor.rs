//! Per-machine actors for background mutations (`NodeOps`) and stale-while-revalidate
//! read projections (`Projected`), so degraded or offline `beep-node` links never
//! block UI commands or stall healthy machines.

use super::{Fleet, alerts, session_lists::SessionLists, session_mirror::SessionMirror};
use crate::error::{DesktopError, Result};
use beep_mesh::{
    PeerClient, RETRY,
    machine::{
        self,
        sessions::{self, NodeWork},
        terminals::{self, Kept as KeptTerminal},
    },
};
use beep_proto::{
    AgentModels, CustomAgent, SessionId, SyncthingPlace, ToolUpdate,
    node::{
        agent::{ReviewRun, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
        device::{
            Device, DeviceId, DeviceState, PROTOCOL as DEVICE_PROTOCOL, Reply as DeviceReply,
            Request as DeviceRequest,
        },
        events::Value,
        harness::HarnessReport,
        mcp_servers::{
            Harness as McpHarness, NativeServer, PROTOCOL as MCP_PROTOCOL, Reply as McpReply,
            Request as McpRequest,
        },
        process::Task,
        terminal::Reply as PtyReply,
        tool_update::{Available, Tool},
    },
};
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use iroh::EndpointId;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::Hash,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{sync::oneshot, time::timeout};

/// Minimum cooldown before re-querying a key whose background refresh failed.
const REFRESH_RETRY_COOL: Duration = Duration::from_secs(2);
/// Default fast window for interactive mutations before handing off to the node actor.
pub(crate) const FAST_JOB_WAIT: Duration = Duration::from_millis(250);
/// Default TTL for queued node jobs if a machine stays offline.
const DEFAULT_JOB_TTL: Duration = Duration::from_secs(600);
/// Shorter TTL for transient process signals.
const PROCESS_JOB_TTL: Duration = Duration::from_secs(60);
/// What a job's waiters hear when it was not delivered before its time ran out.
const NOT_SENT: &str = "the machine did not take it in time";

#[derive(Clone, Debug)]
struct SharedFailure {
    transport: bool,
    message: String,
}

impl From<&DesktopError> for SharedFailure {
    fn from(err: &DesktopError) -> Self {
        Self {
            transport: err.is_transport(),
            message: err.to_string(),
        }
    }
}

impl From<SharedFailure> for DesktopError {
    fn from(err: SharedFailure) -> Self {
        if err.transport {
            Self::TimedOut(err.message)
        } else {
            Self::BadRequest(err.message)
        }
    }
}

type InFlight<V> = Shared<BoxFuture<'static, std::result::Result<V, SharedFailure>>>;

struct Kept<V> {
    at: Instant,
    value: V,
}

struct HeldProjected<K, V> {
    known: HashMap<K, Kept<V>>,
    in_flight: HashMap<K, InFlight<V>>,
    refresh_ids: HashMap<K, u64>,
    next_refresh: u64,
    /// Keys whose initial cold read already waited `first_wait`, so repeat reads return at once
    /// until the in-flight background fetch settles.
    waited: HashSet<K>,
    /// The last ticket handed out for each key, in the order reads and writes began.
    begun: HashMap<K, u64>,
    /// The ticket of the value each key holds: a read that began earlier is dropped.
    stored: HashMap<K, u64>,
}

impl<K, V> Default for HeldProjected<K, V> {
    fn default() -> Self {
        Self {
            known: HashMap::new(),
            in_flight: HashMap::new(),
            refresh_ids: HashMap::new(),
            next_refresh: 0,
            waited: HashSet::new(),
            begun: HashMap::new(),
            stored: HashMap::new(),
        }
    }
}

/// Stale-while-revalidate projection cache keyed by machine (and optional sub-key).
/// Warm reads return in 0ms and trigger deduplicated background refreshes when stale;
/// cold reads cap waiting on degraded nodes and fire `notify` when late answers arrive.
pub(crate) struct Projected<K, V> {
    held: Mutex<HeldProjected<K, V>>,
    fresh: Duration,
    first_wait: Duration,
    cold_limit: Duration,
}

impl<K, V> Projected<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub(crate) fn new(fresh: Duration, first_wait: Duration, cold_limit: Duration) -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::default(),
            fresh,
            first_wait,
            cold_limit,
        })
    }

    pub(crate) fn put(&self, key: K, value: V) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let ticket = Self::ticket(&mut held, &key);
        Self::store(&mut held, key, value, ticket);
    }

    /// A ticket for a read about to start; give it to [`Self::put_begun`] with its value.
    pub(crate) fn begin(&self, key: &K) -> u64 {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        Self::ticket(&mut held, key)
    }

    /// Stores `value` unless a read or write that began after `ticket` was stored first. False
    /// when dropped.
    pub(crate) fn put_begun(&self, key: K, value: V, ticket: u64) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if held.stored.get(&key).copied().unwrap_or(0) >= ticket {
            return false;
        }
        Self::store(&mut held, key, value, ticket);
        true
    }

    fn ticket(held: &mut HeldProjected<K, V>, key: &K) -> u64 {
        let begun = held.begun.entry(key.clone()).or_insert(0);
        *begun += 1;
        *begun
    }

    fn store(held: &mut HeldProjected<K, V>, key: K, value: V, ticket: u64) {
        held.stored.insert(key.clone(), ticket);
        held.in_flight.remove(&key);
        held.refresh_ids.remove(&key);
        held.waited.remove(&key);
        held.known.insert(
            key,
            Kept {
                at: Instant::now(),
                value,
            },
        );
    }

    pub(crate) fn peek(&self, key: &K) -> Option<V> {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .known
            .get(key)
            .map(|kept| kept.value.clone())
    }

    pub(crate) fn edit(&self, key: &K, change: impl FnOnce(&mut V)) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.in_flight.remove(key);
        held.refresh_ids.remove(key);
        if let Some(kept) = held.known.get_mut(key) {
            change(&mut kept.value);
        }
    }

    pub(crate) fn forget(&self, key: &K) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.in_flight.remove(key);
        held.refresh_ids.remove(key);
        held.known.remove(key);
        held.waited.remove(key);
    }

    pub(crate) fn forget_where(&self, pred: impl Fn(&K) -> bool) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.in_flight.retain(|key, _| !pred(key));
        held.refresh_ids.retain(|key, _| !pred(key));
        held.known.retain(|key, _| !pred(key));
        held.waited.retain(|key| !pred(key));
    }

    fn peek_fresh(&self, key: &K, fresh_for: impl FnOnce(&V) -> Duration) -> Option<(V, bool)> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let kept = held.known.get(key)?;
        let limit = fresh_for(&kept.value);
        Some((kept.value.clone(), kept.at.elapsed() >= limit))
    }

    fn start_refresh<F, Fut>(
        self: &Arc<Self>,
        key: K,
        same: impl Fn(&V, &V) -> bool + Send + 'static,
        notify: impl FnOnce() + Send + 'static,
        fetch: F,
    ) -> InFlight<V>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<V>> + Send + 'static,
    {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(running) = held.in_flight.get(&key) {
            return running.clone();
        }

        held.next_refresh = held.next_refresh.wrapping_add(1);
        let refresh_id = held.next_refresh;
        held.refresh_ids.insert(key.clone(), refresh_id);

        let this = self.clone();
        let refresh_key = key.clone();
        let limit = self.cold_limit;
        let fresh = self.fresh;
        let (tx, rx) = oneshot::channel();

        tauri::async_runtime::spawn(async move {
            let outcome = match timeout(limit, fetch()).await {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(err)) => Err(SharedFailure::from(&err)),
                Err(_) => Err(SharedFailure {
                    transport: true,
                    message: "the machine did not answer in time".into(),
                }),
            };

            let should_notify = {
                let mut held = this.held.lock().unwrap_or_else(PoisonError::into_inner);
                let valid = held.refresh_ids.get(&refresh_key) == Some(&refresh_id);
                if valid {
                    held.in_flight.remove(&refresh_key);
                    held.refresh_ids.remove(&refresh_key);
                }
                let was_waited = if !held.in_flight.contains_key(&refresh_key) {
                    held.waited.remove(&refresh_key)
                } else {
                    false
                };

                if !valid {
                    // Invalidated on this key by a newer mutation while in flight: discard.
                    false
                } else {
                    match &outcome {
                        Ok(value) => {
                            let changed = match held.known.get(&refresh_key) {
                                Some(old) => !same(&old.value, value),
                                None => was_waited,
                            };
                            held.known.insert(
                                refresh_key,
                                Kept {
                                    at: Instant::now(),
                                    value: value.clone(),
                                },
                            );
                            changed || was_waited
                        }
                        Err(_) => {
                            if let Some(old) = held.known.get_mut(&refresh_key) {
                                let backoff = fresh.saturating_sub(REFRESH_RETRY_COOL);
                                old.at = Instant::now()
                                    .checked_sub(backoff)
                                    .unwrap_or_else(Instant::now);
                            }
                            false
                        }
                    }
                }
            };

            let _ = tx.send(outcome);

            if should_notify {
                notify();
            }
        });

        let shared = async move {
            rx.await.unwrap_or_else(|_| {
                Err(SharedFailure {
                    transport: true,
                    message: "refresh cancelled".into(),
                })
            })
        }
        .boxed()
        .shared();
        held.in_flight.insert(key, shared.clone());
        shared
    }

    /// Serves a cached value in 0ms (refreshing in the background if stale), or awaits a
    /// deduplicated cold fetch bounded by `cold_limit`.
    pub(crate) async fn read<F, Fut>(
        self: &Arc<Self>,
        key: K,
        same: impl Fn(&V, &V) -> bool + Send + 'static,
        notify: impl FnOnce() + Send + 'static,
        fetch: F,
    ) -> Result<V>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<V>> + Send + 'static,
    {
        self.read_with_fresh(key, |_| self.fresh, same, notify, fetch)
            .await
    }

    pub(crate) async fn read_with_fresh<F, Fut>(
        self: &Arc<Self>,
        key: K,
        fresh_for: impl FnOnce(&V) -> Duration,
        same: impl Fn(&V, &V) -> bool + Send + 'static,
        notify: impl FnOnce() + Send + 'static,
        fetch: F,
    ) -> Result<V>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<V>> + Send + 'static,
    {
        if let Some((cached, stale)) = self.peek_fresh(&key, fresh_for) {
            if stale {
                drop(self.start_refresh(key, same, notify, fetch));
            }

            return Ok(cached);
        }

        self.start_refresh(key, same, notify, fetch)
            .await
            .map_err(DesktopError::from)
    }

    /// Serves a cached value in 0ms, or waits at most `first_wait` on the very first cold read
    /// before returning `None` and letting the background task call `notify` when it finishes.
    pub(crate) async fn read_fast_with_fresh<F, Fut>(
        self: &Arc<Self>,
        key: K,
        fresh_for: impl FnOnce(&V) -> Duration,
        same: impl Fn(&V, &V) -> bool + Send + 'static,
        notify: impl FnOnce() + Send + 'static,
        fetch: F,
    ) -> Option<Result<V>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<V>> + Send + 'static,
    {
        if let Some((cached, stale)) = self.peek_fresh(&key, fresh_for) {
            if stale {
                drop(self.start_refresh(key, same, notify, fetch));
            }

            return Some(Ok(cached));
        }

        {
            let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            if held.in_flight.contains_key(&key) && held.waited.contains(&key) {
                return None;
            }
        }

        let running = self.start_refresh(key.clone(), same, notify, fetch);
        match timeout(self.first_wait, running).await {
            Ok(Ok(value)) => Some(Ok(value)),
            Ok(Err(err)) => Some(Err(DesktopError::from(err))),
            Err(_) => {
                self.held
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .waited
                    .insert(key);
                None
            }
        }
    }

    pub(crate) async fn read_or_default<F, Fut>(
        self: &Arc<Self>,
        key: K,
        default: impl FnOnce() -> V,
        same: impl Fn(&V, &V) -> bool + Send + 'static,
        notify: impl FnOnce() + Send + 'static,
        fetch: F,
    ) -> V
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<V>> + Send + 'static,
    {
        let fresh = self.fresh;
        match self
            .read_fast_with_fresh(key, move |_| fresh, same, notify, fetch)
            .await
        {
            Some(Ok(value)) => value,
            Some(Err(_)) | None => default(),
        }
    }
}

/// Cached Git branch and ahead/behind state for one checkout on one machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct GitStateRow {
    /// `None` on a detached HEAD.
    pub(crate) branch: Option<String>,
    /// `None` before the first push.
    pub(crate) upstream: Option<String>,
    pub(crate) ahead: u32,
    pub(crate) behind: u32,
    /// Why the checkout could not be read.
    pub(crate) error: Option<String>,
}

/// How long a machine has to list its devices: a Mac's `simctl` and
/// `devicectl` take up to about 20 s with nothing attached, plus margin.
pub(crate) const DEVICE_LIST_LIMIT: Duration = Duration::from_secs(35);

/// What a machine's device list holds: its devices, or why the first read of
/// them failed, kept so a window that stopped waiting hears of it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Listing {
    Devices(Vec<Device>),
    Failed(String),
}

/// All per-node read projections shared on [`Fleet`].
pub(crate) struct Projections {
    pub(crate) models: Arc<Projected<(EndpointId, String), AgentModels>>,
    pub(crate) custom_agents: Arc<Projected<(EndpointId, String), Vec<CustomAgent>>>,
    pub(crate) harnesses: Arc<Projected<EndpointId, HarnessReport>>,
    pub(crate) git_state: Arc<Projected<(EndpointId, String), GitStateRow>>,
    pub(crate) devices: Arc<Projected<EndpointId, Listing>>,
    pub(crate) reviews: Arc<Projected<(EndpointId, SessionId), Vec<ReviewRun>>>,
    pub(crate) search: Arc<Projected<EndpointId, String>>,
    /// Syncthing places of a project, cached across sync progress redraws.
    pub(crate) syncthing: Arc<Projected<String, Vec<SyncthingPlace>>>,
    /// What nodes without the `tool-updates` topic answer to the check call.
    pub(crate) tool_updates: Arc<Projected<EndpointId, Vec<ToolUpdate>>>,
    pub(crate) tool_pushes: ToolPushes,
    pub(crate) terminal_pushes: TerminalPushes,
}

/// What a node said on its `tool-updates` topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ToolPush {
    /// The node pushes its list, so the check call is not made.
    Pushed(Vec<Available>),
    /// The node answered the topic with an error, as one that predates it does: its list
    /// comes from the check call.
    Refused,
}

/// The `tool-updates` answer of each machine; a machine not here has not answered yet.
#[derive(Default)]
pub(crate) struct ToolPushes {
    held: Mutex<HashMap<EndpointId, ToolPush>>,
}

impl ToolPushes {
    /// Takes a node's answer on its `tool-updates` topic: its snapshot, or `None` for the error
    /// a node that predates the topic answers. True when the machine's state changed.
    pub(crate) fn heard(&self, machine: EndpointId, answer: Option<&Value>) -> bool {
        let push = match answer {
            // The node has not finished its first check, or runs none.
            Some(Value::Missing) => ToolPush::Pushed(Vec::new()),
            Some(Value::ToolUpdates(found)) => ToolPush::Pushed(found.clone()),
            _ => ToolPush::Refused,
        };

        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(machine, push.clone())
            .is_none_or(|before| before != push)
    }

    pub(crate) fn get(&self, machine: EndpointId) -> Option<ToolPush> {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&machine)
            .cloned()
    }

    /// Drops `tool` from the list `machine` pushed, once it was installed there; the node's
    /// next list replaces it.
    pub(crate) fn installed(&self, machine: EndpointId, tool: Tool) {
        if let Some(ToolPush::Pushed(list)) = self
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&machine)
        {
            list.retain(|update| update.tool != tool);
        }
    }
}

/// The interactive terminals each machine last pushed on its `terminals` topic. A machine
/// not here has pushed none on this connection, so its terminals are asked for.
#[derive(Default)]
pub(crate) struct TerminalPushes {
    held: Mutex<HashMap<EndpointId, Vec<Task>>>,
}

impl TerminalPushes {
    /// Takes the list a node pushed; anything else it said on the topic (an error from a node
    /// that predates it) leaves the machine without one.
    pub(crate) fn heard(&self, machine: EndpointId, value: Option<&Value>) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

        match value {
            Some(Value::Terminals(PtyReply::Terminals { terminals })) => {
                held.insert(machine, terminals.clone());
            }
            _ => {
                held.remove(&machine);
            }
        }
    }

    pub(crate) fn get(&self, machine: EndpointId) -> Option<Vec<Task>> {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&machine)
            .cloned()
    }

    /// The machine's subscription ended: what it pushed may be out of date.
    pub(crate) fn forget(&self, machine: EndpointId) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&machine);
    }
}

impl Default for Projections {
    fn default() -> Self {
        Self {
            models: Projected::new(
                Duration::from_secs(300),
                Duration::from_millis(450),
                Duration::from_secs(6),
            ),
            custom_agents: Projected::new(
                Duration::from_secs(30),
                Duration::from_millis(450),
                Duration::from_secs(5),
            ),
            harnesses: Projected::new(
                Duration::from_secs(300),
                Duration::from_millis(500),
                Duration::from_secs(8),
            ),
            git_state: Projected::new(
                Duration::from_secs(10),
                Duration::from_millis(450),
                Duration::from_secs(5),
            ),
            devices: Projected::new(
                Duration::from_secs(10),
                Duration::from_millis(600),
                DEVICE_LIST_LIMIT + Duration::from_secs(5),
            ),
            reviews: Projected::new(
                Duration::from_secs(8),
                Duration::from_millis(450),
                Duration::from_secs(5),
            ),
            search: Projected::new(
                Duration::from_secs(60),
                Duration::from_millis(450),
                Duration::from_secs(4),
            ),
            syncthing: Projected::new(
                Duration::from_secs(300),
                Duration::from_millis(500),
                Duration::from_secs(8),
            ),
            tool_updates: Projected::new(
                Duration::from_secs(300),
                Duration::from_millis(500),
                Duration::from_secs(8),
            ),
            tool_pushes: ToolPushes::default(),
            terminal_pushes: TerminalPushes::default(),
        }
    }
}

impl Projections {
    pub(crate) fn forget_session(&self, machine: EndpointId, session: &SessionId) {
        self.reviews.forget(&(machine, session.clone()));
    }

    pub(crate) fn forget_models_on(&self, machine: EndpointId) {
        self.models.forget_where(|(m, _)| *m == machine);
    }

    pub(crate) fn forget_models_of(&self, agent: &str) {
        self.models.forget_where(|(_, a)| a == agent);
    }

    pub(crate) fn forget_syncthing(&self, project: &str) {
        self.syncthing.forget(&project.to_owned());
    }

    pub(crate) fn forget_all_syncthing(&self) {
        self.syncthing.forget_where(|_| true);
    }
}

/// Outbound node-level mutation handled asynchronously by a machine's actor.
#[derive(Clone, Debug)]
pub(crate) enum JobKind {
    TerminalStop {
        id: uuid::Uuid,
        task: u32,
        pty: bool,
        command: String,
    },
    DeleteSession {
        agent: String,
        session: SessionId,
    },
    RelocateSession {
        agent: String,
        session: SessionId,
        folder: String,
    },
    StopProcess {
        session: SessionId,
        pid: u32,
    },
    DevicePower {
        id: DeviceId,
        boot: bool,
    },
    InstallAdapters,
    SetSearch {
        search: String,
    },
    ApplyMcpServer {
        harness: McpHarness,
        server: NativeServer,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum JobKey {
    TerminalStop { id: uuid::Uuid },
    DeleteSession { session: SessionId },
    RelocateSession { session: SessionId },
    StopProcess { session: SessionId, pid: u32 },
    DevicePower { id: DeviceId },
    InstallAdapters,
    SetSearch,
    ApplyMcpServer { harness: String, name: String },
}

/// What runs in order on a machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Lane {
    Session(SessionId),
    Key(JobKey),
}

impl JobKind {
    fn key(&self) -> JobKey {
        match self {
            Self::TerminalStop { id, .. } => JobKey::TerminalStop { id: *id },
            Self::DeleteSession { session, .. } => JobKey::DeleteSession {
                session: session.clone(),
            },
            Self::RelocateSession { session, .. } => JobKey::RelocateSession {
                session: session.clone(),
            },
            Self::StopProcess { session, pid } => JobKey::StopProcess {
                session: session.clone(),
                pid: *pid,
            },
            Self::DevicePower { id, .. } => JobKey::DevicePower { id: id.clone() },
            Self::InstallAdapters => JobKey::InstallAdapters,
            Self::SetSearch { .. } => JobKey::SetSearch,
            Self::ApplyMcpServer { harness, server } => JobKey::ApplyMcpServer {
                harness: harness.as_ref().to_owned(),
                name: server.name.clone(),
            },
        }
    }

    /// The jobs of one lane run one after another; those of different lanes together. A
    /// session's delete and move share a lane, so they keep the order they were asked in.
    fn lane(&self) -> Lane {
        match self {
            Self::DeleteSession { session, .. } | Self::RelocateSession { session, .. } => {
                Lane::Session(session.clone())
            }
            _ => Lane::Key(self.key()),
        }
    }

    fn ttl(&self) -> Duration {
        match self {
            Self::StopProcess { .. } => PROCESS_JOB_TTL,
            _ => DEFAULT_JOB_TTL,
        }
    }
}

/// Whether a submitted node job settled during the fast window or stayed queued for retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobOutcome {
    Delivered,
    Queued,
}

type JobWaiter = oneshot::Sender<std::result::Result<(), String>>;

struct QueuedJob {
    id: u64,
    kind: JobKind,
    sending: bool,
    attempts: u32,
    deadline: Instant,
    waiters: Vec<JobWaiter>,
}

#[derive(Default)]
struct HeldOps {
    queues: HashMap<EndpointId, Vec<QueuedJob>>,
    /// The lanes a worker runs now.
    running: HashSet<(EndpointId, Lane)>,
    deleting: HashSet<(EndpointId, SessionId)>,
    relocating: HashMap<(EndpointId, SessionId), String>,
    closing_terminals: HashSet<uuid::Uuid>,
}

/// Per-machine outbound mutation actor: coalesces duplicate actions, gives healthy nodes a
/// fast completion window, and retries degraded or offline nodes in the background.
#[derive(Default)]
pub(crate) struct NodeOps {
    next_id: AtomicU64,
    held: Mutex<HeldOps>,
}

enum JobFailure {
    Transport,
    Refused(String),
}

impl NodeOps {
    pub(crate) fn is_deleting_session(&self, machine_id: &str, session: &SessionId) -> bool {
        let Ok(machine) = machine_id.parse::<EndpointId>() else {
            return false;
        };
        self.is_deleting_session_on(machine, session)
    }

    pub(crate) fn is_deleting_session_on(&self, machine: EndpointId, session: &SessionId) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .deleting
            .contains(&(machine, session.clone()))
    }

    pub(crate) fn relocated_folder(&self, machine_id: &str, session: &SessionId) -> Option<String> {
        let machine = machine_id.parse::<EndpointId>().ok()?;
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .relocating
            .get(&(machine, session.clone()))
            .cloned()
    }

    pub(crate) fn is_closing_terminal(&self, id: uuid::Uuid) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closing_terminals
            .contains(&id)
    }

    /// Submits a job to `machine`'s actor, returning its immediate outcome if the machine answers
    /// within `fast_wait` or `JobOutcome::Queued` if it is slow or offline.
    pub(crate) async fn submit(
        self: &Arc<Self>,
        app: Option<AppHandle>,
        fleet: Arc<Fleet>,
        machine: EndpointId,
        kind: JobKind,
        fast_wait: Duration,
    ) -> Result<JobOutcome> {
        let (tx, rx) = oneshot::channel();
        let lane = kind.lane();

        if self.push_job(machine, kind, Some(tx)) {
            self.spawn_worker(app, fleet, machine, lane);
        }

        match timeout(fast_wait, rx).await {
            Ok(Ok(Ok(()))) => Ok(JobOutcome::Delivered),
            Ok(Ok(Err(why))) => Err(DesktopError::BadRequest(why)),
            Ok(Err(_)) | Err(_) => Ok(JobOutcome::Queued),
        }
    }

    /// Queues `kind`; true when its lane has no worker, which the caller starts.
    fn push_job(&self, machine: EndpointId, kind: JobKind, waiter: Option<JobWaiter>) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);

        match &kind {
            JobKind::TerminalStop { id, .. } => {
                held.closing_terminals.insert(*id);
            }
            JobKind::DeleteSession { session, .. } => {
                held.deleting.insert((machine, session.clone()));
                held.relocating.remove(&(machine, session.clone()));
            }
            JobKind::RelocateSession {
                session, folder, ..
            } => {
                held.relocating
                    .insert((machine, session.clone()), folder.clone());
            }
            _ => {}
        }

        let key = kind.key();
        let lane = kind.lane();
        let deadline = Instant::now() + kind.ttl();
        let queue = held.queues.entry(machine).or_default();

        if let Some(existing) = queue
            .iter_mut()
            .find(|job| !job.sending && job.kind.key() == key)
        {
            existing.kind = kind;
            existing.deadline = deadline;
            if let Some(waiter) = waiter {
                existing.waiters.push(waiter);
            }
        } else {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            queue.push(QueuedJob {
                id,
                kind,
                sending: false,
                attempts: 0,
                deadline,
                waiters: waiter.into_iter().collect(),
            });
        }

        held.running.insert((machine, lane))
    }

    /// The next job of `lane` to send, and the jobs of it that waited in the queue past their
    /// time, which are settled as failed: their waiters hear of it and the caller tells the
    /// windows. With none left to send the lane has no worker.
    fn next_due(
        &self,
        machine: EndpointId,
        lane: &Lane,
    ) -> (Option<(u64, JobKind, u32)>, Vec<JobKind>) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let mut expired = Vec::new();

        if let Some(queue) = held.queues.get_mut(&machine) {
            let mut kept = Vec::new();

            for job in std::mem::take(queue) {
                if job.kind.lane() == *lane && !job.sending && now >= job.deadline {
                    for waiter in job.waiters {
                        let _ = waiter.send(Err(NOT_SENT.to_owned()));
                    }
                    expired.push(job.kind);
                } else {
                    kept.push(job);
                }
            }

            *queue = kept;
        }

        for kind in &expired {
            clear_optimistic(&mut held, machine, kind);
        }

        if let Some(job) = held
            .queues
            .get_mut(&machine)
            .and_then(|queue| queue.iter_mut().find(|job| job.kind.lane() == *lane))
        {
            job.sending = true;
            return (Some((job.id, job.kind.clone(), job.attempts)), expired);
        }

        if held.queues.get(&machine).is_some_and(Vec::is_empty) {
            held.queues.remove(&machine);
        }

        held.running.remove(&(machine, lane.clone()));
        (None, expired)
    }

    fn settle_job(
        &self,
        machine: EndpointId,
        id: u64,
        result: std::result::Result<(), JobFailure>,
    ) -> Option<(JobKind, std::result::Result<(), String>)> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        let queue = held.queues.get_mut(&machine)?;
        let pos = queue.iter().position(|job| job.id == id)?;

        match result {
            Ok(()) => {
                let job = queue.remove(pos);
                clear_optimistic(&mut held, machine, &job.kind);
                for waiter in job.waiters {
                    let _ = waiter.send(Ok(()));
                }
                Some((job.kind, Ok(())))
            }
            Err(JobFailure::Refused(why)) => {
                let job = queue.remove(pos);
                clear_optimistic(&mut held, machine, &job.kind);
                for waiter in job.waiters {
                    let _ = waiter.send(Err(why.clone()));
                }
                Some((job.kind, Err(why)))
            }
            Err(JobFailure::Transport) => {
                let job = &mut queue[pos];
                job.sending = false;
                job.attempts = job.attempts.saturating_add(1);
                // Releasing waiters lets `submit` return `Queued` immediately when offline.
                job.waiters.clear();
                if Instant::now() >= job.deadline {
                    let expired = queue.remove(pos);
                    clear_optimistic(&mut held, machine, &expired.kind);
                    return Some((expired.kind, Err(NOT_SENT.to_owned())));
                }
                None
            }
        }
    }

    fn spawn_worker(
        self: &Arc<Self>,
        app: Option<AppHandle>,
        fleet: Arc<Fleet>,
        machine: EndpointId,
        lane: Lane,
    ) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            this.run_lane(app, fleet, machine, lane).await;
        });
    }

    /// Sends the jobs of one lane of `machine` in order until none is left. Another lane of the
    /// machine runs meanwhile, so a long job, such as a device boot, holds back only its own.
    async fn run_lane(
        self: &Arc<Self>,
        app: Option<AppHandle>,
        fleet: Arc<Fleet>,
        machine: EndpointId,
        lane: Lane,
    ) {
        loop {
            let (due, expired) = self.next_due(machine, &lane);

            for kind in &expired {
                on_settled(app.as_ref(), &fleet, machine, kind, false);
            }

            let Some((id, kind, attempts)) = due else {
                return;
            };
            let outcome = execute_job(&fleet, machine, &kind, attempts).await;

            match self.settle_job(machine, id, outcome) {
                Some((done_kind, settled)) => {
                    on_settled(app.as_ref(), &fleet, machine, &done_kind, settled.is_ok());
                }
                None => {
                    alerts::until_online(&fleet, machine).await;
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
    }
}

fn clear_optimistic(held: &mut HeldOps, machine: EndpointId, kind: &JobKind) {
    match kind {
        JobKind::TerminalStop { id, .. } => {
            held.closing_terminals.remove(id);
        }
        JobKind::DeleteSession { session, .. } => {
            held.deleting.remove(&(machine, session.clone()));
        }
        JobKind::RelocateSession { session, .. } => {
            held.relocating.remove(&(machine, session.clone()));
        }
        _ => {}
    }
}

fn on_settled(
    app: Option<&AppHandle>,
    fleet: &Fleet,
    machine: EndpointId,
    kind: &JobKind,
    ok: bool,
) {
    match kind {
        // The command that asked for the delete took the session out of every list. Only a
        // refusal needs more: the machine still has it.
        JobKind::DeleteSession { .. } if !ok => {
            if let Some(app) = app {
                if let Some(lists) = app.try_state::<SessionLists>() {
                    lists.stale(machine);
                }
                if let Some(mirror) = app.try_state::<Arc<SessionMirror>>() {
                    mirror.delete_failed(machine);
                    // The windows draw at once; the relist follows.
                    super::home_signal::poke(app);
                }
            }
        }
        // The node pushes a move that went through, but a past session is only in the lists'
        // history answer, which still has its old folder. Only a refusal needs the mirror read.
        JobKind::RelocateSession {
            session, folder, ..
        } => {
            if let Some(app) = app {
                if ok && let Some(lists) = app.try_state::<SessionLists>() {
                    lists.relocated(machine, session.clone(), folder.clone());
                }
                if !ok && let Some(mirror) = app.try_state::<Arc<SessionMirror>>() {
                    mirror.moved(machine);
                }
            }
        }
        JobKind::DevicePower { .. } => {
            fleet.projections().devices.forget(&machine);
            if let Some(app) = app {
                let _ = app.emit("devices-changed", ());
            }
        }
        JobKind::SetSearch { .. } if !ok => {
            fleet.projections().search.forget(&machine);
        }
        JobKind::InstallAdapters => {
            fleet.projections().harnesses.forget(&machine);
        }
        JobKind::TerminalStop { .. } if !ok => {
            if let Some(app) = app {
                super::home_signal::poke(app);
            }
        }
        JobKind::DeleteSession { .. }
        | JobKind::TerminalStop { .. }
        | JobKind::StopProcess { .. }
        | JobKind::SetSearch { .. }
        | JobKind::ApplyMcpServer { .. } => {}
    }
}

async fn client_within(
    fleet: &Fleet,
    machine: EndpointId,
    limit: Duration,
) -> std::result::Result<PeerClient, JobFailure> {
    match timeout(limit, fleet.client(machine)).await {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
        Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
        Err(_) => Err(JobFailure::Transport),
    }
}

async fn execute_job(
    fleet: &Fleet,
    machine: EndpointId,
    kind: &JobKind,
    attempts: u32,
) -> std::result::Result<(), JobFailure> {
    match kind {
        JobKind::TerminalStop {
            id,
            task,
            pty,
            command,
        } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let kept = KeptTerminal {
                task: *task,
                pty: *pty,
                command,
            };
            match timeout(Duration::from_secs(5), terminals::stop(&client, &kept)).await {
                Ok(Ok(())) => {
                    super::terminal_rows::forget(fleet, *id)
                        .await
                        .map_err(|_| JobFailure::Transport)?;
                    Ok(())
                }
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(format!(
                    "{}: {err:#}",
                    fleet.name_of(machine)
                ))),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::DeleteSession { agent, session } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(
                Duration::from_secs(8),
                sessions::delete_session(&client, agent, session),
            )
            .await
            {
                Ok(Ok(NodeWork::Done(()))) => Ok(()),
                Ok(Ok(NodeWork::Refused(why))) => {
                    let lower = why.to_ascii_lowercase();
                    if attempts > 0 && (lower.contains("not found") || lower.contains("no such")) {
                        return Ok(());
                    }
                    Err(JobFailure::Refused(format!(
                        "{}: {why}",
                        fleet.name_of(machine)
                    )))
                }
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(format!(
                    "Could not delete the session on {}: {err:#}",
                    fleet.name_of(machine)
                ))),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::RelocateSession {
            agent,
            session,
            folder,
        } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(
                Duration::from_secs(8),
                sessions::relocate_session(&client, agent, session, folder),
            )
            .await
            {
                Ok(Ok(NodeWork::Done(()))) => Ok(()),
                Ok(Ok(NodeWork::Refused(why))) => Err(JobFailure::Refused(why)),
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::StopProcess { session, pid } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let reply = client
                .call_within::<_, SessionsReply>(
                    Duration::from_secs(8),
                    SESSIONS_PROTOCOL,
                    &SessionsRequest::StopProcess {
                        session: session.clone(),
                        pid: *pid,
                    },
                )
                .await;
            match reply {
                Ok(SessionsReply::ProcessStopped {}) => Ok(()),
                Ok(SessionsReply::Error { message }) => {
                    let lower = message.to_ascii_lowercase();
                    if attempts > 0 && (lower.contains("not running") || lower.contains("no such"))
                    {
                        return Ok(());
                    }
                    Err(JobFailure::Refused(message))
                }
                Ok(_) => Err(JobFailure::Refused(
                    "The machine cannot stop running commands yet.".into(),
                )),
                Err(err) if err.is_transport() => Err(JobFailure::Transport),
                Err(err) => Err(JobFailure::Refused(err.to_string())),
            }
        }
        JobKind::DevicePower { id, boot } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let request = if *boot {
                DeviceRequest::Boot { id: id.clone() }
            } else {
                DeviceRequest::Shutdown { id: id.clone() }
            };
            let (_tx, mut rx) = client
                .open_sending(DEVICE_PROTOCOL, &request)
                .await
                .map_err(|err| {
                    if err.is_transport() {
                        JobFailure::Transport
                    } else {
                        JobFailure::Refused(err.to_string())
                    }
                })?;
            match timeout(Duration::from_secs(60), rx.recv::<DeviceReply>()).await {
                Ok(Ok(Some(DeviceReply::Done {}))) => Ok(()),
                Ok(Ok(Some(DeviceReply::Failed { reason } | DeviceReply::Ended { reason }))) => {
                    Err(JobFailure::Refused(reason))
                }
                Ok(Ok(Some(other))) => Err(JobFailure::Refused(format!(
                    "unexpected device reply {other:?}"
                ))),
                Ok(Ok(None)) | Err(_) => Err(JobFailure::Transport),
                Ok(Err(_)) => Err(JobFailure::Transport),
            }
        }
        JobKind::InstallAdapters => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(Duration::from_secs(15), sessions::install_adapters(&client)).await {
                Ok(Ok(true)) => Ok(()),
                Ok(Ok(false)) => Err(JobFailure::Refused(format!(
                    "Update beep-node on {} to manage its coding agents here.",
                    fleet.name_of(machine)
                ))),
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::SetSearch { search } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            match timeout(
                Duration::from_secs(10),
                machine::set_search(&client, search.trim()),
            )
            .await
            {
                Ok(Ok(saved)) => {
                    fleet.projections().search.put(machine, saved);
                    Ok(())
                }
                Ok(Err(err)) if err.is_transport() => Err(JobFailure::Transport),
                Ok(Err(err)) => Err(JobFailure::Refused(err.to_string())),
                Err(_) => Err(JobFailure::Transport),
            }
        }
        JobKind::ApplyMcpServer { harness, server } => {
            let client = client_within(fleet, machine, Duration::from_secs(4)).await?;
            let reply = client
                .call_within::<_, McpReply>(
                    Duration::from_secs(10),
                    MCP_PROTOCOL,
                    &McpRequest::Apply {
                        harness: *harness,
                        server: server.clone(),
                    },
                )
                .await;
            match reply {
                Ok(McpReply::Applied) => Ok(()),
                Ok(McpReply::Refused(err)) => Err(JobFailure::Refused(err)),
                Ok(_) => Err(JobFailure::Refused("Unexpected MCP server reply.".into())),
                Err(beep_mesh::MeshError::Declined { reason, .. })
                    if reason == format!("this node does not offer {MCP_PROTOCOL}") =>
                {
                    Err(JobFailure::Refused(
                        "Update Beep on this machine to manage its MCP servers.".into(),
                    ))
                }
                Err(err) if err.is_transport() => Err(JobFailure::Transport),
                Err(err) => Err(JobFailure::Refused(err.to_string())),
            }
        }
    }
}

/// Optimistically marks `id` booting or off in `machine`'s cached device list.
pub(crate) fn mark_device_power(fleet: &Fleet, machine: EndpointId, id: &DeviceId, boot: bool) {
    fleet.projections().devices.edit(&machine, |listing| {
        let Listing::Devices(devices) = listing else {
            return;
        };

        if let Some(device) = devices.iter_mut().find(|d| d.id == *id) {
            device.state = if boot {
                DeviceState::Booting
            } else {
                DeviceState::Off
            };
        }
    });
}

#[cfg(test)]
#[path = "../tests/fleet_node_actor.rs"]
mod tests;
