//! All windows share one duplex stream per subscription on each demanded node.

use super::{Fleet, Machines, Opened, agents as direct, session_mirror::SessionMirror, terminals};
use crate::error::{DesktopError, Result};
use beep_proto::node::events::{self, CallReply, Operation, Topic};
use beep_proto::watched;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
    time::{MissedTickBehavior, timeout},
};
use uuid::Uuid;

mod commands;
mod snapshots;
mod subscriptions;
use commands::CommandWorker;
#[cfg(test)]
use commands::Registered;
pub(crate) use snapshots::Snapshots;
use subscriptions::SubscriptionWorker;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Interest {
    pub machine: Option<String>,
    pub topic: Topic,
}

#[derive(Clone, Serialize)]
pub struct Update {
    pub(crate) machine_id: String,
    pub(crate) revision: u64,
    pub(crate) topic: Topic,
    pub(crate) query: Option<u64>,
    pub(crate) initial: bool,
    pub(crate) data: Option<serde_json::Value>,
    /// Local order of retained updates, for priming a window with only what it has not seen.
    pub(crate) seq: u64,
}

type Sink = Arc<dyn Fn(Update) -> bool + Send + Sync>;
type Delivery = Arc<Mutex<Sink>>;
struct Window {
    id: Uuid,
    topics: Vec<Interest>,
    _stop: watch::Sender<()>,
    send: Delivery,
    paused: Option<Paused>,
}

/// A hidden window keeps its demand for `Registry::grace`, so showing it again sends only what
/// changed meanwhile instead of reloading every snapshot.
struct Paused {
    at: Instant,
    /// The last update sequence the window had been sent.
    seq: u64,
    /// The topics the window had been sent; topics it takes while paused are primed in full.
    topics: Vec<Interest>,
    /// False once a retained update the window had not seen was dropped.
    complete: bool,
}

impl Window {
    fn live(&self) -> bool {
        self.paused.is_none()
    }

    /// Whether the window still holds node subscriptions and retained updates.
    fn demands(&self, grace: Duration) -> bool {
        self.paused
            .as_ref()
            .is_none_or(|paused| paused.at.elapsed() < grace)
    }
}

/// How long a hidden window keeps its node streams. A phone is suspended soon after it hides.
#[cfg(mobile)]
const PAUSE_GRACE: Duration = Duration::from_secs(60);
#[cfg(not(mobile))]
const PAUSE_GRACE: Duration = Duration::from_secs(300);

/// What a window that shows again or reattaches was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Primed {
    /// Only updates it had not seen.
    Changes,
    /// It was hidden past the grace period, so its streams start again.
    Expired,
    /// Everything held, after held updates were lost, and every stream sends its state again.
    Full,
}

struct Pending {
    window: String,
    id: Uuid,
    topics: Option<Vec<Interest>>,
}

type SnapshotKey = (String, Topic, Option<u64>);
const MAX_NODE_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

struct CachedSnapshot {
    update: Update,
    bytes: usize,
    used: Instant,
}

#[derive(Default)]
struct Latest {
    entries: BTreeMap<SnapshotKey, CachedSnapshot>,
    bytes: usize,
    /// The sequence of the last retained update.
    seq: u64,
    /// The sequence when a retained update was last dropped before being replaced.
    lost: u64,
}

impl Latest {
    fn values(&self) -> impl Iterator<Item = &Update> {
        self.entries.values().map(|entry| &entry.update)
    }

    /// Whether the last update under `key` carried `data`.
    fn holds(&self, key: &SnapshotKey, data: &serde_json::Value) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry.update.data.as_ref() == Some(data))
    }

    fn remove(&mut self, key: &SnapshotKey) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
        }
    }

    fn retain(&mut self, mut keep: impl FnMut(&SnapshotKey, &Update) -> bool) {
        self.entries.retain(|key, entry| keep(key, &entry.update));
        self.bytes = self.entries.values().map(|entry| entry.bytes).sum();
    }

    fn resources(&mut self, machine: &str, scope: &Topic, resources: &[Topic]) {
        if scope.name() != "files" || !scope.is_pattern() {
            return;
        }

        let present: BTreeSet<_> = resources.iter().collect();
        self.retain(|(source, topic, query), _| {
            source != machine
                || query.is_some()
                || topic.is_pattern()
                || !scope.covers(topic)
                || present.contains(topic)
        });
    }

    fn clear_node(&mut self, machine: &str, controller: bool) {
        self.retain(|(source, _, _), update| {
            source != machine || controller && controller_topic(&update.topic)
        });
    }

    /// Removes what no remaining interest receives and returns it.
    fn drop_unwanted(
        &mut self,
        interests: &[Interest],
        controller: Option<&EndpointId>,
    ) -> Vec<Update> {
        let mut removed = Vec::new();
        self.entries.retain(|_, entry| {
            let keep = receives(interests, &entry.update, controller);

            if !keep {
                removed.push(entry.update.clone());
            }

            keep
        });
        self.bytes = self.entries.values().map(|entry| entry.bytes).sum();

        if !removed.is_empty() {
            self.lost = self.seq;
        }

        removed
    }

    /// Stamps the next sequence on `update` and returns it.
    fn stamp(&mut self, update: &mut Update) -> u64 {
        self.seq = self.seq.saturating_add(1);
        update.seq = self.seq;
        self.seq
    }

    /// Retains `update` and returns the updates it pushed out of the memory budget, itself
    /// included when it is too large to keep.
    fn insert(&mut self, update: Update, bytes: usize) -> Vec<Update> {
        let key = (
            update.machine_id.clone(),
            update.topic.clone(),
            update.query,
        );
        self.remove(&key);

        if bytes > MAX_NODE_SNAPSHOT_BYTES {
            self.lost = self.seq;
            return vec![update];
        }

        let mut evicted = Vec::new();

        loop {
            let node_bytes: usize = self
                .entries
                .values()
                .filter(|entry| entry.update.machine_id == update.machine_id)
                .map(|entry| entry.bytes)
                .sum();
            let node_full = node_bytes.saturating_add(bytes) > MAX_NODE_SNAPSHOT_BYTES;

            if !node_full
                && self.bytes.saturating_add(bytes) <= MAX_SNAPSHOT_BYTES
                && self.entries.len() < 4096
            {
                break;
            }

            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| !node_full || entry.update.machine_id == update.machine_id)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                self.lost = self.seq;
                return vec![update];
            };

            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                evicted.push(entry.update);
            }
        }

        if !evicted.is_empty() {
            self.lost = self.seq;
        }

        self.bytes += bytes;
        self.entries.insert(
            key,
            CachedSnapshot {
                update,
                bytes,
                used: Instant::now(),
            },
        );
        evicted
    }
}

struct Registry {
    windows: HashMap<String, Window>,
    pending: VecDeque<Pending>,
    latest: Latest,
    grace: Duration,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            windows: HashMap::new(),
            pending: VecDeque::new(),
            latest: Latest::default(),
            grace: PAUSE_GRACE,
        }
    }
}

impl Registry {
    /// Windows that still hold node demand: shown, or hidden within the grace period.
    fn demanding(&self) -> impl Iterator<Item = &Window> {
        self.windows
            .values()
            .filter(|window| window.demands(self.grace))
    }

    /// Windows that are sent updates and keep them: the demanding ones but the tray's standing
    /// interest, which only keeps the node streams on and takes none of what they push.
    fn receiving(&self) -> impl Iterator<Item = &Window> {
        self.windows
            .iter()
            .filter(|(label, window)| !standing(label) && window.demands(self.grace))
            .map(|(_, window)| window)
    }

    /// A paused window that had not been sent a dropped update must be primed in full.
    fn note_lost(&mut self, removed: &[Update], controller: Option<&EndpointId>) {
        for window in self.windows.values_mut() {
            if let Some(paused) = window.paused.as_mut()
                && removed
                    .iter()
                    .any(|update| receives(&paused.topics, update, controller))
            {
                paused.complete = false;
            }
        }
    }

    fn remember(&mut self, window: &str, id: Uuid, topics: Option<Vec<Interest>>) {
        self.pending
            .retain(|entry| entry.window != window || entry.id != id);
        if self.pending.len() == 256 {
            self.pending.pop_front();
        }

        self.pending.push_back(Pending {
            window: window.to_owned(),
            id,
            topics,
        });
    }

    fn register(
        &mut self,
        window: &str,
        id: Uuid,
        mut topics: Vec<Interest>,
        send: Sink,
    ) -> Result<Option<watch::Receiver<()>>> {
        if let Some(at) = self
            .pending
            .iter()
            .position(|entry| entry.window == window && entry.id == id)
        {
            let Some(wanted) = self.pending.remove(at).and_then(|entry| entry.topics) else {
                return Ok(None);
            };
            topics = wanted;
        }

        self.check_topics(window, &topics)?;
        let (stop, stopped) = watch::channel(());
        self.windows.insert(
            window.to_owned(),
            Window {
                id,
                topics,
                _stop: stop,
                send: Arc::new(Mutex::new(send)),
                paused: None,
            },
        );
        Ok(Some(stopped))
    }

    fn remove(&mut self, window: &str, id: Uuid) {
        if self
            .windows
            .get(window)
            .is_some_and(|current| current.id == id)
        {
            self.windows.remove(window);
        }

        self.remember(window, id, None);
    }

    fn topics(&mut self, window: &str, id: Uuid, topics: Vec<Interest>) -> Result<()> {
        if self
            .pending
            .iter()
            .any(|entry| entry.window == window && entry.id == id && entry.topics.is_none())
        {
            return Ok(());
        }

        if self
            .windows
            .get(window)
            .is_some_and(|current| current.id == id)
        {
            self.check_topics(window, &topics)?;
        }

        if let Some(current) = self.windows.get_mut(window)
            && current.id == id
        {
            current.topics = topics;
        } else {
            self.remember(window, id, Some(topics));
        }

        Ok(())
    }

    /// A rejected reader must not make every existing node stream exceed its wire bound.
    fn check_topics(&self, window: &str, wanted: &[Interest]) -> Result<()> {
        let mut global = BTreeSet::new();
        let mut scoped: BTreeMap<&str, BTreeSet<Topic>> = BTreeMap::new();

        for interest in self
            .windows
            .iter()
            .filter(|(label, _)| label.as_str() != window)
            .flat_map(|(_, held)| held.topics.iter())
            .chain(wanted)
        {
            match interest.machine.as_deref() {
                Some(machine) => {
                    scoped
                        .entry(machine)
                        .or_default()
                        .insert(interest.topic.clone());
                }
                None => {
                    global.insert(interest.topic.clone());
                }
            }
        }

        let exceeds = |topics: BTreeSet<Topic>| {
            compact_topics(topics.into_iter().collect()).len() > events::MAX_TOPICS
        };

        if exceeds(global.clone())
            || scoped.into_values().any(|mut topics| {
                topics.extend(global.iter().cloned());
                exceeds(topics)
            })
        {
            return Err(DesktopError::BadRequest(
                "too many topic subscriptions for one node across open windows".into(),
            ));
        }

        Ok(())
    }

    fn interests(&self) -> Vec<Interest> {
        let mut topics: Vec<_> = self
            .demanding()
            .flat_map(|window| window.topics.clone())
            .collect();
        topics.sort();
        topics.dedup();
        topics
    }
}

struct Call {
    operation: Operation,
    reply: oneshot::Sender<std::result::Result<CallReply, String>>,
}
type Commands = Arc<Mutex<HashMap<EndpointId, (Uuid, mpsc::Sender<Call>)>>>;

pub(crate) struct Events {
    registry: Mutex<Registry>,
    demand: watch::Sender<Vec<Interest>>,
    refresh: watch::Sender<()>,
    commands: Commands,
    manager: Mutex<Option<Job>>,
    source: Mutex<Option<Source>>,
    controller: OnceLock<EndpointId>,
    revision: Mutex<u64>,
}

impl Default for Events {
    fn default() -> Self {
        Self {
            registry: Mutex::default(),
            demand: watch::channel(Vec::new()).0,
            refresh: watch::channel(()).0,
            commands: Commands::default(),
            manager: Mutex::default(),
            source: Mutex::default(),
            controller: OnceLock::new(),
            revision: Mutex::new(0),
        }
    }
}

struct Job(tokio::task::JoinHandle<()>);
impl Drop for Job {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Source {
    active: watch::Sender<bool>,
    _task: Job,
}

impl Events {
    pub(crate) fn clear(&self, window: &str) {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);

        for pending in registry
            .pending
            .iter_mut()
            .filter(|pending| pending.window == window)
        {
            pending.topics = None;
        }

        if let Some(current) = registry.windows.get(window) {
            let id = current.id;
            registry.remove(window, id);
        }

        drop(registry);
        self.changed();
    }

    /// Changes a window's topics and primes it with what is held for the new ones, as a window
    /// that opens is, since an unchanged view is not sent again.
    fn retopic(&self, window: &str, id: Uuid, topics: Vec<Interest>) -> Result<()> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        // A paused window is primed for the topics it took when it shows again.
        let before = registry
            .windows
            .get(window)
            .filter(|held| held.id == id && held.live())
            .map(|held| held.topics.clone());
        registry.topics(window, id, topics)?;

        if let Some(before) = before
            && let Some(held) = registry.windows.get(window)
        {
            let controller = self.controller.get();
            let primed: Vec<_> = registry
                .latest
                .values()
                .filter(|update| {
                    receives(&held.topics, update, controller)
                        && !receives(&before, update, controller)
                })
                .cloned()
                .collect();
            let delivery = held.send.clone();
            // Hold the delivery gate so a live update cannot overtake the primed ones.
            let send = delivery.lock().unwrap_or_else(PoisonError::into_inner);
            drop(registry);
            let failed = primed.into_iter().any(|update| !send(update));
            drop(send);

            if failed {
                self.remove(window, id);
                return Ok(());
            }
        } else {
            drop(registry);
        }

        self.changed();
        Ok(())
    }

    fn remove(&self, window: &str, id: Uuid) {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(window, id);
        self.changed();
    }

    fn changed(&self) {
        let controller = self.controller.get();
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let interests = registry.interests();
        let removed = registry.latest.drop_unwanted(&interests, controller);
        registry.note_lost(&removed, controller);
        self.publish(&registry);
    }

    /// Stops sending to a hidden window while keeping its demand and what it was sent.
    /// Returns whether it was shown before, so the caller can time the grace period.
    fn pause(&self, window: &str, id: Uuid, topics: Option<Vec<Interest>>) -> Result<bool> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let seq = registry.latest.seq;
        let Some(held) = registry
            .windows
            .get_mut(window)
            .filter(|held| held.id == id)
        else {
            return Ok(false);
        };
        let paused = held.paused.is_none();

        if paused {
            held.paused = Some(Paused {
                at: Instant::now(),
                seq,
                topics: held.topics.clone(),
                complete: true,
            });
        }

        if let Some(topics) = topics {
            registry.topics(window, id, topics)?;
        }

        drop(registry);
        self.changed();
        Ok(paused)
    }

    /// Shows a paused window again, sending only the held updates it has not seen, unless
    /// some were lost or its grace period ended.
    fn resume(
        &self,
        window: &str,
        id: Uuid,
        topics: Option<Vec<Interest>>,
    ) -> Result<Option<Primed>> {
        let controller = self.controller.get();
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let grace = registry.grace;
        let paused = registry
            .windows
            .get_mut(window)
            .filter(|held| held.id == id)
            .map(|held| (held.paused.take(), held.topics.clone()));
        let Some((paused, current)) = paused else {
            if let Some(topics) = topics {
                registry.topics(window, id, topics)?;
            }

            drop(registry);
            self.changed();
            return Ok(None);
        };

        if let Some(topics) = topics
            && let Err(error) = registry.topics(window, id, topics)
        {
            if let Some(held) = registry.windows.get_mut(window) {
                held.paused = paused;
            }

            return Err(error);
        }

        let primed = match &paused {
            Some(paused) if paused.at.elapsed() >= grace => Primed::Expired,
            Some(paused) if !paused.complete => Primed::Full,
            _ => Primed::Changes,
        };
        let (before, since) = match paused {
            Some(paused) if primed != Primed::Full => (paused.topics, Some(paused.seq)),
            Some(_) => (Vec::new(), None),
            None => (current, None),
        };
        let Some(held) = registry.windows.get(window) else {
            return Ok(None);
        };
        let updates: Vec<_> = registry
            .latest
            .values()
            .filter(|update| {
                receives(&held.topics, update, controller)
                    && (!receives(&before, update, controller)
                        || since.is_some_and(|since| update.seq > since))
            })
            .cloned()
            .collect();
        let delivery = held.send.clone();
        // Hold the delivery gate so a live update cannot overtake the primed ones.
        let send = delivery.lock().unwrap_or_else(PoisonError::into_inner);
        drop(registry);
        let failed = updates.into_iter().any(|update| !send(update));
        drop(send);

        if failed {
            self.remove(window, id);
            return Ok(None);
        }

        self.changed();

        if primed == Primed::Full {
            self.refresh.send_replace(());
        }

        Ok(Some(primed))
    }

    /// Keep publication inside the registry lock so concurrent IPC changes cannot publish stale demand.
    fn publish(&self, registry: &Registry) {
        let topics = registry.interests();
        // Hosts without a push feed are polled only for shown windows.
        let forge = registry
            .windows
            .values()
            .filter(|window| window.live())
            .flat_map(|window| &window.topics)
            .any(|interest| matches!(interest.topic.name(), "forge" | "*"));
        self.demand.send_if_modified(|held| {
            if *held == topics {
                false
            } else {
                *held = topics;
                true
            }
        });

        if let Some(source) = self
            .source
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            source.active.send_if_modified(|held| {
                let changed = *held != forge;
                *held = forge;
                changed
            });
        }
    }

    fn register(
        &self,
        window: &str,
        id: Uuid,
        topics: Vec<Interest>,
        send: Sink,
        alive: impl Fn() -> bool,
        since: Option<u64>,
    ) -> Result<Option<(watch::Receiver<()>, Primed)>> {
        if !alive() {
            self.remove(window, id);
            return Ok(None);
        }

        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(stopped) = registry.register(window, id, topics, send)? else {
            return Ok(None);
        };
        let held = registry
            .windows
            .get(window)
            .ok_or_else(|| DesktopError::BadRequest("the topic reader was removed".into()))?;
        // A reader that ends and attaches again with the same topics passes the last sequence
        // it was sent. It needs only newer updates, unless held ones were dropped since.
        let since =
            since.filter(|since| registry.latest.lost <= *since && *since <= registry.latest.seq);
        let delivery = held.send.clone();
        // Prime a new window even if another window already owns exactly these interests.
        // Hold its delivery gate until priming is sent so live events cannot overtake it.
        let send = delivery.lock().unwrap_or_else(PoisonError::into_inner);
        let initial: Vec<_> = registry
            .latest
            .values()
            .filter(|update| {
                receives(&held.topics, update, self.controller.get())
                    && since.is_none_or(|since| update.seq > since)
            })
            .cloned()
            .collect();
        self.publish(&registry);
        drop(registry);

        if !alive() {
            drop(send);
            self.remove(window, id);
            return Ok(None);
        }

        let failed = initial.into_iter().any(|update| !send(update));
        drop(send);

        if failed {
            self.remove(window, id);
            return Ok(None);
        }

        if since.is_some() {
            return Ok(Some((stopped, Primed::Changes)));
        }

        self.refresh.send_replace(());
        Ok(Some((stopped, Primed::Full)))
    }

    /// A standing interest in `topics` that no window owns, so it is never paused: the nodes keep
    /// pushing them while every window is hidden, until the returned reader is dropped.
    #[cfg(desktop)]
    pub(crate) fn hold(&self, who: &str, topics: Vec<Interest>) -> Result<Reading<'_>> {
        let id = Uuid::new_v4();
        let label = standing_label(who);
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .register(&label, id, topics, Arc::new(|_| true))?;
        self.changed();

        Ok(Reading {
            events: self,
            window: label,
            id,
        })
    }

    /// Replaces what `who` holds with `topics`; none ends it. Like `hold`, it keeps the streams
    /// on and takes none of what they push.
    #[cfg(desktop)]
    pub(crate) fn hold_set(&self, who: &str, topics: Vec<Interest>) -> Result<()> {
        let label = standing_label(who);
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);

        if topics.is_empty() {
            registry.windows.remove(&label);
        } else {
            registry.register(&label, Uuid::new_v4(), topics, Arc::new(|_| true))?;
        }

        drop(registry);
        self.changed();
        Ok(())
    }

    pub(crate) fn start(&self, app: AppHandle, fleet: Arc<Fleet>) {
        self.controller.get_or_init(|| fleet.me);
        let mut manager = self.manager.lock().unwrap_or_else(PoisonError::into_inner);

        if manager.as_ref().is_none_or(|job| job.0.is_finished()) {
            *manager = Some(Job(tokio::spawn(run(
                app.clone(),
                fleet,
                self.demand.subscribe(),
                self.commands.clone(),
            ))));
        }

        drop(manager);
        let mut source = self.source.lock().unwrap_or_else(PoisonError::into_inner);

        if source.is_none() {
            let (active, changes) = watch::channel(false);
            *source = Some(Source {
                active,
                _task: Job(tokio::spawn(forge_source(app, changes))),
            });
        }

        drop(source);
        self.changed();
    }

    /// Retains `update` for every window that wants it and sends it to the shown ones. Hidden
    /// windows within their grace period are sent it once they show.
    pub(crate) fn dispatch(&self, mut update: Update) {
        let controller = self.controller.get();
        let bytes = serde_json::to_vec(&update).map_or(usize::MAX, |bytes| bytes.len());
        let destinations: Vec<_> = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);

            if !registry
                .receiving()
                .any(|window| receives(&window.topics, &update, controller))
            {
                return;
            }

            registry.latest.stamp(&mut update);
            let removed = registry.latest.insert(update.clone(), bytes);
            registry.note_lost(&removed, controller);
            let destinations: Vec<_> = registry
                .windows
                .iter()
                .filter(|(_, window)| {
                    window.live() && receives(&window.topics, &update, controller)
                })
                .map(|(label, window)| (label.clone(), window.id, window.send.clone()))
                .collect();

            if !destinations.is_empty() {
                metrics::counter!("beep_ipc_emit_total", "topic" => update.topic.name().to_owned())
                    .increment(1);
            }

            destinations
        };

        for (label, id, delivery) in destinations {
            let sent = {
                let send = delivery.lock().unwrap_or_else(PoisonError::into_inner);
                send(update.clone())
            };

            if !sent {
                self.remove(&label, id);
            }
        }
    }

    pub(crate) fn dispatch_node(&self, update: Update) {
        let aliases: BTreeSet<_> = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .receiving()
            .flat_map(|window| &window.topics)
            .filter(|interest| {
                interest
                    .machine
                    .as_ref()
                    .is_none_or(|machine| machine == &update.machine_id)
            })
            .filter(|interest| {
                matches!(
                    (update.topic.name(), interest.topic.name()),
                    ("agents", "agent") | ("processes", "process-output")
                )
            })
            .map(|interest| interest.topic.clone())
            .collect();
        self.dispatch(update.clone());

        for topic in aliases {
            self.dispatch(Update {
                topic,
                ..update.clone()
            });
        }
    }

    /// The terminals whose screen a window follows.
    pub(crate) fn terminals_followed(&self) -> BTreeSet<Uuid> {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .receiving()
            .flat_map(|window| &window.topics)
            .filter_map(|interest| terminals::followed(&interest.topic))
            .collect()
    }

    pub(crate) fn node_wanted(&self, machine: EndpointId, topic: &Topic) -> bool {
        let id = machine.to_string();
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .receiving()
            .flat_map(|window| &window.topics)
            .any(|interest| {
                interest
                    .machine
                    .as_ref()
                    .is_none_or(|machine| machine == &id)
                    && (interest.topic.matches(topic)
                        || matches!(
                            (topic.name(), interest.topic.name()),
                            ("agents", "agent") | ("processes", "process-output")
                        ))
            })
    }

    fn forget_queries(&self, machine: EndpointId, queries: &[events::Query]) {
        let machine = machine.to_string();
        let ids: BTreeSet<_> = queries.iter().map(|query| query.id).collect();
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .retain(|(source, _, query), _| {
                source != &machine || query.is_none_or(|query| ids.contains(&query))
            });
    }

    fn clear_node(&self, machine: EndpointId) {
        let controller = self.controller.get() == Some(&machine);
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .clear_node(&machine.to_string(), controller);
    }

    fn resources(&self, machine: EndpointId, scope: &Topic, resources: &[Topic]) {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .resources(&machine.to_string(), scope, resources);
    }

    /// Whether a shown window wants the controller view `topic`, so it is drawn now. The standing
    /// interests keep the streams on and take no view.
    pub(crate) fn controller_wanted(&self, topic: &Topic) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .windows
            .iter()
            .filter(|(label, window)| !standing(label) && window.live())
            .flat_map(|(_, window)| &window.topics)
            .any(|interest| interest.machine.is_none() && interest.topic.matches(topic))
    }

    /// Whether a shown or briefly hidden window wants the controller view `topic`, so a
    /// drawn view is kept for it.
    pub(crate) fn controller_held(&self, topic: &Topic) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .receiving()
            .flat_map(|window| &window.topics)
            .any(|interest| interest.machine.is_none() && interest.topic.matches(topic))
    }

    /// Sends a rendered view to the windows that want it. One the same as the last sent is
    /// dropped: the windows hold it, and a window that opens is primed with it.
    pub(crate) fn publish_views(&self, machine: EndpointId, topic: Topic, data: serde_json::Value) {
        if !self.controller_held(&topic) {
            return;
        }

        let key = (machine.to_string(), topic, None);
        if self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .holds(&key, &data)
        {
            return;
        }

        let (_, topic, _) = key;

        let revision = {
            let mut revision = self.revision.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(next) = revision.checked_add(1) else {
                return;
            };
            *revision = next;
            next
        };
        self.dispatch(Update {
            machine_id: machine.to_string(),
            revision,
            topic,
            query: None,
            initial: false,
            data: Some(data),
            seq: 0,
        });
    }

    /// Sends the views read for a node's topic as an update of that topic, with the revision the
    /// node gave, to the windows that follow it.
    pub(crate) fn push_node_views(
        &self,
        machine: EndpointId,
        topic: Topic,
        revision: u64,
        data: serde_json::Value,
    ) {
        if !self.node_wanted(machine, &topic) {
            return;
        }

        self.dispatch(Update {
            machine_id: machine.to_string(),
            revision,
            topic,
            query: None,
            initial: false,
            data: Some(data),
            seq: 0,
        });
    }

    pub(crate) async fn call(
        &self,
        window: &str,
        machine: EndpointId,
        operation: Operation,
    ) -> Result<Option<CallReply>> {
        if !self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .windows
            .contains_key(window)
        {
            return Ok(None);
        }

        self.call_machine(machine, operation, Duration::from_secs(125))
            .await
    }

    pub(crate) async fn call_machine(
        &self,
        machine: EndpointId,
        operation: Operation,
        limit: Duration,
    ) -> Result<Option<CallReply>> {
        let sender = self
            .commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&machine)
            .map(|(_, sender)| sender.clone());
        let Some(sender) = sender else {
            return Ok(None);
        };
        let (reply, result) = oneshot::channel();
        timeout(
            Duration::from_secs(5),
            sender.send(Call { operation, reply }),
        )
        .await
        .map_err(|_| DesktopError::Lost("the machine".into()))?
        .map_err(|_| DesktopError::Lost("the machine".into()))?;
        let result = timeout(limit, result)
            .await
            .map_err(|_| DesktopError::Unsure("the machine".into()))?
            .map_err(|_| DesktopError::Unsure("the machine".into()))?;
        result.map(Some).map_err(DesktopError::BadRequest)
    }
}

/// The start of the registry label of a standing interest, which no window can have.
const STANDING: &str = "standing:";

#[cfg(desktop)]
fn standing_label(who: &str) -> String {
    format!("{STANDING}{who}")
}

/// Whether `label` is a standing interest's, which keeps streams on and takes no updates.
fn standing(label: &str) -> bool {
    label.starts_with(STANDING)
}

pub(crate) struct Reading<'a> {
    events: &'a Events,
    window: String,
    id: Uuid,
}
impl Drop for Reading<'_> {
    fn drop(&mut self) {
        self.events.remove(&self.window, self.id);
    }
}

/// Changes a reader's topics, or ends it when `topics` is null. With `paused`, a hidden window
/// stops receiving updates but keeps its streams for a grace period, and showing it again
/// sends only what changed.
#[tauri::command]
pub async fn mesh_topics(
    app: AppHandle,
    window: WebviewWindow,
    events: State<'_, Events>,
    id: Uuid,
    topics: Option<Vec<Interest>>,
    paused: Option<bool>,
) -> Result<()> {
    if let Some(topics) = &topics {
        validate(topics)?;
    }

    let label = window.label();

    match (paused, topics) {
        (None, Some(topics)) => events.retopic(label, id, topics)?,
        (None, None) => events.remove(label, id),
        (Some(true), topics) => {
            if events.pause(label, id, topics)? {
                let grace = events
                    .registry
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .grace;
                tokio::spawn(async move {
                    tokio::time::sleep(grace).await;
                    app.state::<Events>().changed();
                });
            }
        }
        (Some(false), topics) => match events.resume(label, id, topics)? {
            Some(Primed::Full) => super::event_views::changed(app, Topic::new("*")),
            Some(Primed::Changes | Primed::Expired) => super::event_views::resumed(app),
            None => {}
        },
    }

    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri passes each command argument separately.
pub async fn mesh_watch(
    app: AppHandle,
    window: WebviewWindow,
    fleet: Machines<'_>,
    events: State<'_, Events>,
    id: Uuid,
    topics: Vec<Interest>,
    on_event: Channel<Update>,
    since: Option<u64>,
) -> Result<()> {
    validate(&topics)?;
    let send: Sink = Arc::new(move |update| on_event.send(update).is_ok());
    events.controller.get_or_init(|| fleet.me);
    let label = window.label();
    let Some((mut stopped, primed)) = events.register(
        label,
        id,
        topics,
        send,
        || app.get_webview_window(label).is_some(),
        since,
    )?
    else {
        return Ok(());
    };
    let _reading = Reading {
        events: &events,
        window: window.label().to_owned(),
        id,
    };
    events.start(app.clone(), fleet.inner().clone());

    // Retained controller views can be evicted while another window stays subscribed.
    if primed == Primed::Full {
        super::event_views::changed(app, Topic::new("*"));
    }

    let _ = stopped.changed().await;
    Ok(())
}

fn validate(topics: &[Interest]) -> Result<()> {
    if topics
        .iter()
        .any(|interest| interest.machine.is_some() && controller_topic(&interest.topic))
    {
        return Err(DesktopError::BadRequest(
            "controller-owned topics cannot specify a machine".into(),
        ));
    }

    if topics.len() > events::MAX_TOPICS
        || topics.iter().any(|interest| {
            !interest.topic.valid()
                || interest
                    .machine
                    .as_ref()
                    .is_some_and(|machine| machine.parse::<EndpointId>().is_err())
        })
    {
        return Err(DesktopError::BadRequest(
            "invalid subscription topics".into(),
        ));
    }

    Ok(())
}

fn controller_topic(topic: &Topic) -> bool {
    matches!(
        topic.name(),
        "machines" | "telemetry" | "projects" | "forge" | "home" | "accounts" | "usage" | "tools"
    )
}

/// Controller topics contain the controller's aggregate state and are never machine-scoped.
fn receives(interests: &[Interest], update: &Update, controller: Option<&EndpointId>) -> bool {
    interests.iter().any(|interest| {
        let source_matches = if controller_topic(&interest.topic) {
            controller.is_some_and(|local| local.to_string() == update.machine_id)
        } else {
            interest
                .machine
                .as_ref()
                .is_none_or(|machine| machine == &update.machine_id)
        };
        source_matches && interest.topic.matches(&update.topic)
    })
}

fn compact_topics(mut topics: Vec<Topic>) -> Vec<Topic> {
    if topics.iter().any(|topic| topic.name() == "home") {
        topics.extend(home_sources());
    }

    let topics: BTreeSet<_> = topics.into_iter().collect();
    topics
        .iter()
        .filter(|topic| {
            !topics
                .iter()
                .any(|other| other != *topic && other.covers(topic))
        })
        .cloned()
        .collect()
}

fn topics_for(interests: &[Interest], machine: EndpointId, local: EndpointId) -> Vec<Topic> {
    let id = machine.to_string();
    let mut topics: Vec<_> = interests
        .iter()
        .filter(|interest| {
            interest.machine.as_ref().is_none_or(|wanted| wanted == &id)
                && (machine == local || !controller_topic(&interest.topic))
                && interest.topic.name() != "tools"
        })
        .map(|interest| interest.topic.clone())
        .collect();
    if machine != local
        && interests
            .iter()
            .any(|interest| interest.machine.is_none() && interest.topic.name() == "home")
    {
        topics.extend(home_sources());
    }

    // The tools view lists what each node found, so every node is asked for it.
    if interests
        .iter()
        .any(|interest| interest.machine.is_none() && interest.topic.name() == "tools")
    {
        topics.push(Topic::new(TOOL_UPDATES));
    }

    topics.sort();
    topics.dedup();
    compact_topics(topics)
}

/// The node topic of the tools it found a newer release of.
const TOOL_UPDATES: &str = "tool-updates";

fn home_sources() -> [Topic; 3] {
    [
        Topic::new("agents"),
        Topic::new("processes"),
        Topic::new("moves"),
    ]
}

fn mesh_is_open(mesh: &watch::Receiver<Opened>) -> bool {
    watched::read(mesh, |opened| matches!(opened, Opened::Open(_)))
}

async fn run(
    app: AppHandle,
    fleet: Arc<Fleet>,
    mut demand: watch::Receiver<Vec<Interest>>,
    commands: Commands,
) {
    let mut mesh = fleet.mesh.subscribe();
    let mut members = fleet.mesh().map(|mesh| mesh.updates());
    let mut status = fleet.status.changes();
    let mut local = beep_node::events::system().subscribe();
    let mut revision = watched::read_and_update(&mut status, |status| *status);
    let mut materialized = Vec::new();
    let mut workers = JoinSet::new();
    let mut command_workers: BTreeMap<EndpointId, tokio::task::AbortHandle> = BTreeMap::new();
    let mut subscriptions: BTreeMap<(EndpointId, Topic), tokio::task::AbortHandle> =
        BTreeMap::new();

    loop {
        while workers.try_join_next().is_some() {}
        let interests = watched::read_and_update(&mut demand, Clone::clone);

        if interests.is_empty() {
            command_workers.clear();
            subscriptions.clear();
            workers.abort_all();
            fleet.snapshots.reconcile(&BTreeMap::new());
            materialized.clear();

            if watched::wait_for(&mut demand, |interests| !interests.is_empty())
                .await
                .is_err()
            {
                return;
            }
            revision = watched::read_and_update(&mut status, |status| *status);
            continue;
        }

        // A window follows a terminal by its id, its node knows it by number.
        let interests =
            terminals::node_interests(interests, &terminals::places(&fleet), |machine| {
                terminals::old_node(&fleet, machine)
            });
        let mut machines = if mesh_is_open(&mesh) {
            fleet.online()
        } else {
            Vec::new()
        };
        machines.push(fleet.me);
        machines.sort();
        machines.dedup();
        let wanted: BTreeMap<_, _> = machines
            .into_iter()
            .filter_map(|machine| {
                let topics = topics_for(&interests, machine, fleet.me);
                (!topics.is_empty()).then_some((machine, topics))
            })
            .collect();
        fleet.snapshots.reconcile(&wanted);
        command_workers.retain(|machine, task| {
            if wanted.contains_key(machine) {
                true
            } else {
                task.abort();
                false
            }
        });
        subscriptions.retain(|(machine, topic), task| {
            if wanted
                .get(machine)
                .is_some_and(|topics| topics.contains(topic))
            {
                true
            } else {
                task.abort();
                false
            }
        });

        for (machine, topics) in wanted {
            // A phone has no node to ask for its own events: a worker for it
            // would fail and retry for as long as the app runs.
            if machine == fleet.me && fleet.session.is_none() {
                continue;
            }

            command_workers.entry(machine).or_insert_with(|| {
                app.state::<Events>().clear_node(machine);
                workers.spawn(
                    CommandWorker {
                        fleet: fleet.clone(),
                        machine,
                        commands: commands.clone(),
                    }
                    .run(),
                )
            });

            for topic in topics {
                subscriptions
                    .entry((machine, topic.clone()))
                    .or_insert_with(|| {
                        workers.spawn(
                            SubscriptionWorker {
                                app: app.clone(),
                                fleet: fleet.clone(),
                                machine,
                                topic,
                            }
                            .run(),
                        )
                    });
            }
        }

        if materialized != interests {
            super::event_views::changed(app.clone(), Topic::new("*"));
            app.state::<Events>().publish_views(
                fleet.me,
                Topic::new("telemetry"),
                telemetry(&fleet),
            );
            materialized = interests.clone();
        }

        tokio::select! {
            changed = demand.changed() => if changed.is_err() { return; },
            changed = mesh.changed() => {
                if changed.is_err() { return; }
                watched::read_and_update(&mut mesh, |_| ());
                command_workers.clear(); subscriptions.clear(); workers.abort_all();
                app.state::<Events>().registry.lock().unwrap_or_else(PoisonError::into_inner).latest.retain(|(machine, _, _), _| machine == &fleet.me.to_string());
                fleet.snapshots.reconcile(&BTreeMap::new());
                members = fleet.mesh().map(|mesh| mesh.updates());
                beep_node::events::system().changed(Topic::new("projects"));
                beep_node::events::system().changed(Topic::new("accounts"));
            },
            changed = async { match &mut members { Some(members) => members.changed().await, None => std::future::pending().await } } => {
                if changed.is_err() { members = None; }
                crate::projects::team_live::mesh_changed(&app);
                beep_node::events::system().changed(Topic::new("projects"));
                beep_node::events::system().changed(Topic::new("accounts"));
            },
            changed = status.changed() => {
                if changed.is_err() { return; }
                let current = watched::read_and_update(&mut status, |status| *status);

                if current.machines != revision.machines {
                    beep_node::events::system().changed(Topic::new("machines"));
                    beep_node::events::system().changed(Topic::new("accounts"));
                }

                if current.telemetry != revision.telemetry && interests.iter().any(|interest| matches!(interest.topic.name(), "telemetry" | "*")) {
                    beep_node::events::system().changed(Topic::new("telemetry"));
                }
                revision = current;
            },
            changed = local.recv() => match changed {
                Ok(change) if change.topic.name() == "telemetry" => {
                    app.state::<Events>().publish_views(fleet.me, change.topic, telemetry(&fleet));
                },
                // This node's sync status also comes as a snapshot of its own sync stream, which tells
                // a transfer's progress from its end.
                Ok(change) if change.topic.name() == "sync" => {
                    super::event_views::sync_changed(app.clone(), true);
                },
                Ok(change) if matches!(change.topic.name(), "machines" | "tools" | "projects" | "home" | "usage" | "accounts") => {
                    super::event_views::changed(app.clone(), change.topic);
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    super::event_views::changed(app.clone(), Topic::new("*"));
                    app.state::<Events>().publish_views(fleet.me, Topic::new("telemetry"), telemetry(&fleet));
                },
                _ => {},
            },
            Some(result) = workers.join_next() => if let Err(error) = result { log::debug!("event reader stopped: {error}"); },
        }
    }
}

/// Drops what a snapshot of `topic` from `machine` makes out of date. One session's live report
/// or the processes leave the session lists as they are; a live report updates its session.
fn invalidate(app: &AppHandle, machine: EndpointId, topic: &Topic, value: Option<&events::Value>) {
    match topic.name() {
        // One machine's list changed: its pushed rows replace the ones held. Without them
        // the next read lists that machine again.
        "agents" | "*" => {
            let pushed = match value {
                Some(value) if topic.name() == "agents" => {
                    direct::pushed_sessions(&app.state::<Arc<Fleet>>(), machine, value)
                }
                _ => None,
            };

            match pushed {
                Some(rows) => app.state::<Arc<SessionMirror>>().pushed(machine, rows),
                None => app.state::<Arc<SessionMirror>>().stale(machine),
            }
        }
        "agent" => {
            if let Some(events::Value::Sessions(reply)) = value
                && let beep_proto::node::agent::SessionsReply::Live { status } = reply.as_ref()
            {
                app.state::<Arc<SessionMirror>>()
                    .reported(&machine.to_string(), &status.session);
            }
        }
        _ => {}
    }
}

/// Hosts without a push feed are read once for all windows. Results arrive as IPC events.
async fn forge_source(app: AppHandle, mut active: watch::Receiver<bool>) {
    loop {
        if watched::wait_for(&mut active, |active| *active)
            .await
            .is_err()
        {
            return;
        }

        let mut previous = None;
        let mut ticks = tokio::time::interval(Duration::from_secs(60));
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                changed = active.changed() => {
                    if changed.is_err() { return; }
                    if !watched::read_and_update(&mut active, |active| *active) { break; }
                },
                _ = ticks.tick() => {
                    let read = crate::projects::git_items::git_items(
                        app.state::<Arc<beep_fleet::Session>>(),
                        app.state::<Arc<crate::center::view_cache::ViewCache>>());
                    let result = tokio::select! {
                        result = read => result,
                        _ = watched::wait_for(&mut active, |active| !*active) => break,
                    };

                    match result {
                        Ok(Some(items)) => {
                            let current = serde_json::to_value(&items).ok();

                            if current != previous {
                                previous = current;
                                let fleet = app.state::<Arc<Fleet>>();
                                app.state::<Events>().publish_views(fleet.me, Topic::new("forge"),
                                    serde_json::json!({ "views": [{ "command": "git_items", "args": {}, "value": items }] }));
                            }
                        },
                        Ok(None) => {},
                        Err(error) => {
                            previous = None;
                            let fleet = app.state::<Arc<Fleet>>();
                            let view = super::event_views::View::failed("git_items", serde_json::json!({}), &error.to_string());
                            app.state::<Events>().publish_views(fleet.me, Topic::new("forge"), serde_json::json!({ "views": [view] }));
                        },
                    }
                }
            }
        }
    }
}

fn telemetry(fleet: &Fleet) -> serde_json::Value {
    let stats: Vec<_> = fleet
        .status
        .stats()
        .into_iter()
        .map(|sample| {
            serde_json::json!({
                "id": sample.id,
                "sampled_at": crate::beep::epoch_ms(sample.sampled_at),
                "disks": sample.disks,
                "memory_used_mb": sample.memory_used_mb,
                "memory_total_mb": sample.memory_total_mb,
                "load": sample.load,
            })
        })
        .collect();
    serde_json::json!({ "stats": stats, "traffic": fleet.status.traffic() })
}

#[cfg(test)]
#[path = "../tests/fleet_events.rs"]
mod tests;
