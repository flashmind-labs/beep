//! What the desktop remembers of each node's answers.

use crate::error::{DesktopError, Result};
use beep_proto::{
    AgentModels, CustomAgent, SessionId, SyncthingPlace, ToolUpdate,
    node::{
        agent::ReviewRun,
        device::Device,
        events::Value,
        harness::HarnessReport,
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
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::{sync::oneshot, time::timeout};

/// Minimum cooldown before re-querying a key whose background refresh failed.
const REFRESH_RETRY_COOL: Duration = Duration::from_secs(2);

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
        if let Some((cached, stale)) = self.peek_fresh(&key, |_| self.fresh) {
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

/// All per-node read projections shared on `Fleet`.
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

#[cfg(test)]
#[path = "../tests/fleet_projections.rs"]
mod tests;
