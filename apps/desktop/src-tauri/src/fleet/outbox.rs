//! What the windows follow and are sent: the registry of their readers, what they want, and the
//! updates kept for them. It sits below the node intake (`events`) and the views, which both
//! send through it and follow what it publishes.

use super::{
    terminal_rows,
    view_signal::{Interest, Update, Wants, aliased, join_changed},
};
use crate::error::{DesktopError, Result};
use beep_proto::{
    node::events::{self, Topic},
    watched,
};
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;

pub(crate) type Sink = Arc<dyn Fn(Update) -> bool + Send + Sync>;
pub(crate) type Delivery = Arc<Mutex<Sink>>;
pub(crate) struct Window {
    pub(crate) id: Uuid,
    pub(crate) topics: Vec<Interest>,
    pub(crate) _stop: watch::Sender<()>,
    pub(crate) send: Delivery,
    pub(crate) paused: Option<Paused>,
}

/// A hidden window keeps its demand for `Registry::grace`, so showing it again sends only what
/// changed meanwhile instead of reloading every snapshot.
pub(crate) struct Paused {
    pub(crate) at: Instant,
    /// The last update sequence the window had been sent.
    pub(crate) seq: u64,
    /// The topics the window had been sent; topics it takes while paused are primed in full.
    pub(crate) topics: Vec<Interest>,
    /// False once a retained update the window had not seen was dropped.
    pub(crate) complete: bool,
}

impl Window {
    pub(crate) fn live(&self) -> bool {
        self.paused.is_none()
    }

    /// Whether the window still holds node subscriptions and retained updates.
    pub(crate) fn demands(&self, grace: Duration) -> bool {
        self.paused
            .as_ref()
            .is_none_or(|paused| paused.at.elapsed() < grace)
    }
}

/// How long a hidden window keeps its node streams. A phone is suspended soon after it hides.
#[cfg(mobile)]
pub(crate) const PAUSE_GRACE: Duration = Duration::from_secs(60);
#[cfg(not(mobile))]
pub(crate) const PAUSE_GRACE: Duration = Duration::from_secs(300);

/// What a window that shows again or reattaches was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Primed {
    /// Only updates it had not seen.
    Changes,
    /// It was hidden past the grace period, so its streams start again.
    Expired,
    /// Everything held, after held updates were lost, and every stream sends its state again.
    Full,
}

pub(crate) struct Pending {
    pub(crate) window: String,
    pub(crate) id: Uuid,
    pub(crate) topics: Option<Vec<Interest>>,
}

pub(crate) type SnapshotKey = (String, Topic, Option<u64>);
pub(crate) const MAX_NODE_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

pub(crate) struct CachedSnapshot {
    update: Update,
    bytes: usize,
    used: Instant,
}

#[derive(Default)]
pub(crate) struct Latest {
    pub(crate) entries: BTreeMap<SnapshotKey, CachedSnapshot>,
    pub(crate) bytes: usize,
    /// The sequence of the last retained update.
    pub(crate) seq: u64,
    /// The sequence when a retained update was last dropped before being replaced.
    pub(crate) lost: u64,
}

impl Latest {
    pub(crate) fn values(&self) -> impl Iterator<Item = &Update> {
        self.entries.values().map(|entry| &entry.update)
    }

    /// Whether the last update under `key` carried `data`.
    pub(crate) fn holds(&self, key: &SnapshotKey, data: &serde_json::Value) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry.update.data.as_ref() == Some(data))
    }

    pub(crate) fn remove(&mut self, key: &SnapshotKey) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
        }
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&SnapshotKey, &Update) -> bool) {
        self.entries.retain(|key, entry| keep(key, &entry.update));
        self.bytes = self.entries.values().map(|entry| entry.bytes).sum();
    }

    pub(crate) fn resources(&mut self, machine: &str, scope: &Topic, resources: &[Topic]) {
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

    pub(crate) fn clear_node(&mut self, machine: &str, controller: bool) {
        self.retain(|(source, _, _), update| {
            source != machine || controller && controller_topic(&update.topic)
        });
    }

    /// Removes what no remaining interest receives and returns it.
    pub(crate) fn drop_unwanted(
        &mut self,
        interests: &[Interest],
        controller: &EndpointId,
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
    pub(crate) fn stamp(&mut self, update: &mut Update) -> u64 {
        self.seq = self.seq.saturating_add(1);
        update.seq = self.seq;
        self.seq
    }

    /// Retains `update` and returns the updates it pushed out of the memory budget, itself
    /// included when it is too large to keep.
    pub(crate) fn insert(&mut self, mut update: Update, mut bytes: usize) -> Vec<Update> {
        let key = (
            update.machine_id.clone(),
            update.topic.clone(),
            update.query,
        );

        // A window shown later gets the last update only: it names the paths of those it replaces.
        if update.topic.name() == "files"
            && let Some(earlier) = self.entries.get(&key)
        {
            join_changed(&earlier.update, &mut update);
            bytes = bytes.max(serde_json::to_vec(&update).map_or(usize::MAX, |bytes| bytes.len()));
        }

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

pub(crate) struct Registry {
    pub(crate) windows: HashMap<String, Window>,
    pub(crate) pending: VecDeque<Pending>,
    pub(crate) latest: Latest,
    pub(crate) grace: Duration,
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
    pub(crate) fn demanding(&self) -> impl Iterator<Item = &Window> {
        self.windows
            .values()
            .filter(|window| window.demands(self.grace))
    }

    /// Windows that are sent updates and keep them: the demanding ones but the tray's standing
    /// interest, which only keeps the node streams on and takes none of what they push.
    pub(crate) fn receiving(&self) -> impl Iterator<Item = &Window> {
        self.windows
            .iter()
            .filter(|(label, window)| !standing(label) && window.demands(self.grace))
            .map(|(_, window)| window)
    }

    /// What the windows want now, for `Views`.
    pub(crate) fn wants(&self) -> Wants {
        let shown = self
            .windows
            .iter()
            .filter(|(label, window)| !standing(label) && window.live())
            .flat_map(|(_, window)| window.topics.iter().cloned())
            .collect();
        let held = self
            .receiving()
            .flat_map(|window| window.topics.iter().cloned())
            .collect();

        Wants::new(self.windows.keys().cloned().collect(), shown, held)
    }

    /// A paused window that had not been sent a dropped update must be primed in full.
    pub(crate) fn note_lost(&mut self, removed: &[Update], controller: &EndpointId) {
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

    pub(crate) fn remember(&mut self, window: &str, id: Uuid, topics: Option<Vec<Interest>>) {
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

    pub(crate) fn register(
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

    pub(crate) fn remove(&mut self, window: &str, id: Uuid) {
        if self
            .windows
            .get(window)
            .is_some_and(|current| current.id == id)
        {
            self.windows.remove(window);
        }

        self.remember(window, id, None);
    }

    pub(crate) fn topics(&mut self, window: &str, id: Uuid, topics: Vec<Interest>) -> Result<()> {
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
    pub(crate) fn check_topics(&self, window: &str, wanted: &[Interest]) -> Result<()> {
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

    pub(crate) fn interests(&self) -> Vec<Interest> {
        let mut topics: Vec<_> = self
            .demanding()
            .flat_map(|window| window.topics.clone())
            .collect();
        topics.sort();
        topics.dedup();
        topics
    }
}

pub(crate) struct WindowOutbox {
    pub(crate) registry: Mutex<Registry>,
    pub(crate) demand: watch::Sender<Vec<Interest>>,
    /// What the windows want, for `Views` to follow.
    pub(crate) wants: watch::Sender<Arc<Wants>>,
    pub(crate) refresh: watch::Sender<()>,
    /// Whether a window wants the forge items, for `projects::git_items` to follow.
    pub(crate) forge: watch::Sender<bool>,
    pub(crate) controller: EndpointId,
    pub(crate) revision: AtomicU64,
}

impl WindowOutbox {
    pub(crate) fn new(controller: EndpointId) -> Self {
        Self {
            registry: Mutex::default(),
            demand: watch::channel(Vec::new()).0,
            wants: watch::channel(Arc::default()).0,
            refresh: watch::channel(()).0,
            forge: watch::channel(false).0,
            controller,
            revision: AtomicU64::new(0),
        }
    }

    pub(crate) fn demand(&self) -> watch::Receiver<Vec<Interest>> {
        self.demand.subscribe()
    }

    pub(crate) fn refresh(&self) -> watch::Receiver<()> {
        self.refresh.subscribe()
    }

    pub(crate) fn grace(&self) -> Duration {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .grace
    }

    /// Whether a reader of `window` is attached, hidden or not.
    pub(crate) fn has_window(&self, window: &str) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .windows
            .contains_key(window)
    }

    /// Forgets what other machines pushed, as the mesh swaps.
    pub(crate) fn keep_only(&self, me: EndpointId) {
        let me = me.to_string();
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .retain(|(machine, _, _), _| machine == &me);
    }

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
    pub(crate) fn retopic(&self, window: &str, id: Uuid, topics: Vec<Interest>) -> Result<()> {
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
            let controller = &self.controller;
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

    pub(crate) fn remove(&self, window: &str, id: Uuid) {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(window, id);
        self.changed();
    }

    pub(crate) fn changed(&self) {
        let controller = &self.controller;
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let interests = registry.interests();
        let removed = registry.latest.drop_unwanted(&interests, controller);
        registry.note_lost(&removed, controller);
        self.publish(&registry);
    }

    /// Stops sending to a hidden window while keeping its demand and what it was sent.
    /// Returns whether it was shown before, so the caller can time the grace period.
    pub(crate) fn pause(
        &self,
        window: &str,
        id: Uuid,
        topics: Option<Vec<Interest>>,
    ) -> Result<bool> {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let seq = registry.latest.seq;

        if !registry
            .windows
            .get(window)
            .is_some_and(|held| held.id == id)
        {
            return Ok(false);
        }

        if let Some(topics) = topics {
            registry.topics(window, id, topics)?;
        }

        let Some(held) = registry.windows.get_mut(window) else {
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

        drop(registry);
        self.changed();
        Ok(paused)
    }

    /// Shows a paused window again, sending only the held updates it has not seen, unless
    /// some were lost or its grace period ended.
    pub(crate) fn resume(
        &self,
        window: &str,
        id: Uuid,
        topics: Option<Vec<Interest>>,
    ) -> Result<Option<Primed>> {
        let controller = &self.controller;
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
        let unkept = primed == Primed::Full
            && has_unkept(
                &held.topics,
                &registry.latest.values().collect::<Vec<_>>(),
                controller,
            );
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

        if unkept {
            self.refresh.send_replace(());
        }

        Ok(Some(primed))
    }

    /// Keep publication inside the registry lock so concurrent IPC changes cannot publish stale demand.
    pub(crate) fn publish(&self, registry: &Registry) {
        let topics = registry.interests();
        // Hosts without a push feed are polled only for shown windows.
        let forge = registry
            .windows
            .values()
            .filter(|window| window.live())
            .flat_map(|window| &window.topics)
            .any(|interest| matches!(interest.topic.name(), "forge" | "*"));
        let wants = registry.wants();
        self.wants.send_if_modified(|held| {
            if **held == wants {
                false
            } else {
                *held = Arc::new(wants);
                true
            }
        });
        self.demand.send_if_modified(|held| {
            if *held == topics {
                false
            } else {
                *held = topics;
                true
            }
        });

        self.forge.send_if_modified(|held| {
            let changed = *held != forge;
            *held = forge;
            changed
        });
    }

    pub(crate) fn register(
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
                receives(&held.topics, update, &self.controller)
                    && since.is_none_or(|since| update.seq > since)
            })
            .cloned()
            .collect();
        let unkept = since.is_none()
            && has_unkept(
                &held.topics,
                &registry.latest.values().collect::<Vec<_>>(),
                &self.controller,
            );
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

        if unkept {
            self.refresh.send_replace(());
        }

        Ok(Some((stopped, Primed::Full)))
    }

    /// A standing interest of `who` in `topics` that no window owns, so it is never paused: the
    /// nodes keep pushing them while every window is hidden. It replaces what `who` held; none
    /// ends it. It keeps the streams on and takes none of what they push.
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

    /// Retains `update` for every window that wants it and sends it to the shown ones. Hidden
    /// windows within their grace period are sent it once they show.
    pub(crate) fn dispatch(&self, mut update: Update) {
        let controller = &self.controller;
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
            .filter(|interest| aliased(&update.topic, &interest.topic))
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
            .filter_map(|interest| terminal_rows::followed(&interest.topic))
            .collect()
    }

    /// The windows that have a reader attached, hidden ones too.
    /// What the windows want, as of the last change to them.
    pub(crate) fn wants(&self) -> watch::Receiver<Arc<Wants>> {
        self.wants.subscribe()
    }

    /// Whether a window wants the forge items.
    pub(crate) fn forge(&self) -> watch::Receiver<bool> {
        self.forge.subscribe()
    }

    pub(crate) fn forget_queries(&self, machine: EndpointId, queries: &[events::Query]) {
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

    pub(crate) fn clear_node(&self, machine: EndpointId) {
        let controller = self.controller == machine;
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .clear_node(&machine.to_string(), controller);
    }

    pub(crate) fn resources(&self, machine: EndpointId, scope: &Topic, resources: &[Topic]) {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .resources(&machine.to_string(), scope, resources);
    }

    /// Sends a rendered view to the windows that want it. One the same as the last sent is
    /// dropped: the windows hold it, and a window that opens is primed with it.
    pub(crate) fn publish_views(&self, machine: EndpointId, topic: Topic, data: serde_json::Value) {
        if !watched::read_sent(&self.wants, |wants| wants.controller_held(&topic)) {
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

        let revision = self.revision.fetch_add(1, Ordering::Relaxed) + 1;
        self.dispatch(pushed(machine, revision, topic, data));
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
        if !watched::read_sent(&self.wants, |wants| wants.node_wanted(machine, &topic)) {
            return;
        }

        self.dispatch(pushed(machine, revision, topic, data));
    }
}

/// What a window is sent when `machine` pushes `data` on `topic`.
fn pushed(machine: EndpointId, revision: u64, topic: Topic, data: serde_json::Value) -> Update {
    Update {
        machine_id: machine.to_string(),
        revision,
        topic,
        query: None,
        initial: false,
        data: Some(data),
        seq: 0,
    }
}

/// The start of the registry label of a standing interest, which no window can have.
pub(crate) const STANDING: &str = "standing:";

#[cfg(desktop)]
pub(crate) fn standing_label(who: &str) -> String {
    format!("{STANDING}{who}")
}

/// Whether `label` is a standing interest's, which keeps streams on and takes no updates.
pub(crate) fn standing(label: &str) -> bool {
    label.starts_with(STANDING)
}

/// Ends a window's reader when dropped.
pub(crate) struct Reading<'a> {
    pub(crate) events: &'a WindowOutbox,
    pub(crate) window: String,
    pub(crate) id: Uuid,
}
impl Drop for Reading<'_> {
    fn drop(&mut self) {
        self.events.remove(&self.window, self.id);
    }
}
pub(crate) fn validate(topics: &[Interest]) -> Result<()> {
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

pub(crate) fn controller_topic(topic: &Topic) -> bool {
    matches!(
        topic.name(),
        "machines" | "telemetry" | "projects" | "forge" | "home" | "accounts" | "usage" | "tools"
    )
}

/// Whether some of `interests` has no kept update in `latest`: a refresh then asks the nodes for
/// it. Everything kept was primed from the cache and needs none.
pub(crate) fn has_unkept(
    interests: &[Interest],
    latest: &[&Update],
    controller: &EndpointId,
) -> bool {
    interests.iter().any(|interest| {
        !latest
            .iter()
            .any(|update| receives(std::slice::from_ref(interest), update, controller))
    })
}

/// Controller topics contain the controller's aggregate state and are never machine-scoped.
pub(crate) fn receives(interests: &[Interest], update: &Update, controller: &EndpointId) -> bool {
    interests.iter().any(|interest| {
        let source_matches = if controller_topic(&interest.topic) {
            controller.to_string() == update.machine_id
        } else {
            interest
                .machine
                .as_ref()
                .is_none_or(|machine| machine == &update.machine_id)
        };
        source_matches && interest.topic.matches(&update.topic)
    })
}

pub(crate) fn compact_topics(mut topics: Vec<Topic>) -> Vec<Topic> {
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
pub(crate) fn home_sources() -> [Topic; 3] {
    [
        Topic::new("agents"),
        Topic::new("processes"),
        Topic::new("moves"),
    ]
}
