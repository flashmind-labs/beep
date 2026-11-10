//! All windows share one duplex stream per subscription on each demanded node.

use super::{Fleet, Machines, Opened};
use crate::error::{DesktopError, Result};
use beep_proto::node::events::{self, CallReply, Operation, Topic};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
    time::timeout,
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
}

type Sink = Arc<dyn Fn(Update) -> bool + Send + Sync>;
type Delivery = Arc<Mutex<Sink>>;
struct Window {
    id: Uuid,
    topics: Vec<Interest>,
    _stop: watch::Sender<()>,
    send: Delivery,
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
    used: std::time::Instant,
}

#[derive(Default)]
struct Latest {
    entries: BTreeMap<SnapshotKey, CachedSnapshot>,
    bytes: usize,
}

impl Latest {
    fn values(&self) -> impl Iterator<Item = &Update> {
        self.entries.values().map(|entry| &entry.update)
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

    fn insert(&mut self, update: Update, bytes: usize) {
        let key = (
            update.machine_id.clone(),
            update.topic.clone(),
            update.query,
        );
        self.remove(&key);

        if bytes > MAX_NODE_SNAPSHOT_BYTES {
            return;
        }

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
            let Some(oldest) = oldest else { return };
            self.remove(&oldest);
        }

        self.bytes += bytes;
        self.entries.insert(
            key,
            CachedSnapshot {
                update,
                bytes,
                used: std::time::Instant::now(),
            },
        );
    }
}

#[derive(Default)]
struct Registry {
    windows: HashMap<String, Window>,
    pending: VecDeque<Pending>,
    latest: Latest,
}

impl Registry {
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

        if let Some(current) = self
            .windows
            .get_mut(window)
            .filter(|current| current.id == id)
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
            .windows
            .values()
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

    fn remove(&self, window: &str, id: Uuid) {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(window, id);
        self.changed();
    }

    fn changed(&self) {
        let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let interests = registry.interests();
        registry
            .latest
            .retain(|_, update| receives(&interests, update, self.controller.get()));
        self.publish(&registry);
    }

    /// Keep publication inside the registry lock so concurrent IPC changes cannot publish stale demand.
    fn publish(&self, registry: &Registry) {
        let topics = registry.interests();
        let forge = topics
            .iter()
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
        _data: Option<serde_json::Value>,
    ) -> Result<Option<watch::Receiver<()>>> {
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
        let delivery = held.send.clone();
        // Prime a new window even if another window already owns exactly these interests.
        // Hold its delivery gate until priming is sent so live events cannot overtake it.
        let send = delivery.lock().unwrap_or_else(PoisonError::into_inner);
        let initial: Vec<_> = registry
            .latest
            .values()
            .filter(|update| receives(&held.topics, update, self.controller.get()))
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

        self.refresh.send_replace(());
        Ok(Some(stopped))
    }

    fn start(&self, app: AppHandle, fleet: Arc<Fleet>) {
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

    pub(crate) fn dispatch(&self, update: Update) {
        let bytes = serde_json::to_vec(&update).map_or(usize::MAX, |bytes| bytes.len());
        let destinations: Vec<_> = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);

            if !registry
                .windows
                .values()
                .any(|window| receives(&window.topics, &update, self.controller.get()))
            {
                return;
            }

            metrics::counter!("beep_ipc_emit_total", "topic" => update.topic.name().to_owned())
                .increment(1);
            registry.latest.insert(update.clone(), bytes);
            registry
                .windows
                .iter()
                .filter(|(_, window)| receives(&window.topics, &update, self.controller.get()))
                .map(|(label, window)| (label.clone(), window.id, window.send.clone()))
                .collect()
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

    fn dispatch_node(&self, update: Update) {
        let aliases: BTreeSet<_> = self
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .windows
            .values()
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
                    ("agents", "agent")
                        | ("processes", "process-output")
                        | ("terminals", "terminal-output")
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

    fn node_wanted(&self, machine: EndpointId, topic: &Topic) -> bool {
        let id = machine.to_string();
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .windows
            .values()
            .flat_map(|window| &window.topics)
            .any(|interest| {
                interest
                    .machine
                    .as_ref()
                    .is_none_or(|machine| machine == &id)
                    && (interest.topic.matches(topic)
                        || matches!(
                            (topic.name(), interest.topic.name()),
                            ("agents", "agent")
                                | ("processes", "process-output")
                                | ("terminals", "terminal-output")
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

    pub(crate) fn controller_wanted(&self, topic: &Topic) -> bool {
        self.registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .windows
            .values()
            .flat_map(|window| &window.topics)
            .any(|interest| interest.machine.is_none() && interest.topic.matches(topic))
    }

    pub(crate) fn publish_views(&self, machine: EndpointId, topic: Topic, data: serde_json::Value) {
        if !self.controller_wanted(&topic) {
            return;
        }

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
            Duration::from_secs(15),
            sender.send(Call { operation, reply }),
        )
        .await
        .map_err(|_| DesktopError::Lost("the machine".into()))?
        .map_err(|_| DesktopError::Lost("the machine".into()))?;
        let result = timeout(Duration::from_secs(125), result)
            .await
            .map_err(|_| DesktopError::Unsure("the machine".into()))?
            .map_err(|_| DesktopError::Unsure("the machine".into()))?;
        result.map(Some).map_err(DesktopError::BadRequest)
    }
}

struct Reading<'a> {
    events: &'a Events,
    window: String,
    id: Uuid,
}
impl Drop for Reading<'_> {
    fn drop(&mut self) {
        self.events.remove(&self.window, self.id);
    }
}

#[tauri::command]
pub fn mesh_topics(
    window: WebviewWindow,
    events: State<'_, Events>,
    id: Uuid,
    topics: Option<Vec<Interest>>,
) -> Result<()> {
    if let Some(topics) = topics {
        validate(&topics)?;
        events
            .registry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .topics(window.label(), id, topics)?;
        events.changed();
    } else {
        events.remove(window.label(), id);
    }

    Ok(())
}

#[tauri::command]
pub async fn mesh_command(
    window: WebviewWindow,
    events: State<'_, Events>,
    machine: String,
    operation: Operation,
) -> Result<CallReply> {
    let machine = machine
        .parse()
        .map_err(|_| DesktopError::BadRequest("invalid machine".into()))?;
    events
        .call(window.label(), machine, operation)
        .await?
        .ok_or_else(|| DesktopError::Lost("the machine".into()))
}

#[tauri::command]
pub async fn mesh_watch(
    app: AppHandle,
    window: WebviewWindow,
    fleet: Machines<'_>,
    events: State<'_, Events>,
    id: Uuid,
    topics: Vec<Interest>,
    on_event: Channel<Update>,
) -> Result<()> {
    validate(&topics)?;
    let send: Sink = Arc::new(move |update| on_event.send(update).is_ok());
    events.controller.get_or_init(|| fleet.me);
    let label = window.label();
    let Some(mut stopped) = events.register(
        label,
        id,
        topics,
        send,
        || app.get_webview_window(label).is_some(),
        Some(telemetry(&fleet)),
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
    super::event_views::changed(app, Topic::new("*"));
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
        "machines" | "telemetry" | "projects" | "forge" | "home" | "accounts" | "usage"
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

    topics.sort();
    topics.dedup();
    compact_topics(topics)
}

fn home_sources() -> [Topic; 3] {
    [
        Topic::new("agents"),
        Topic::new("processes"),
        Topic::new("moves"),
    ]
}

fn mesh_is_open(mesh: &watch::Receiver<Opened>) -> bool {
    matches!(*mesh.borrow(), Opened::Open(_))
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
    let mut revision = *status.borrow_and_update();
    let mut materialized = Vec::new();
    let mut workers = JoinSet::new();
    let mut command_workers: BTreeMap<EndpointId, tokio::task::AbortHandle> = BTreeMap::new();
    let mut subscriptions: BTreeMap<(EndpointId, Topic), tokio::task::AbortHandle> =
        BTreeMap::new();

    loop {
        while workers.try_join_next().is_some() {}
        let interests = demand.borrow_and_update().clone();

        if interests.is_empty() {
            command_workers.clear();
            subscriptions.clear();
            workers.abort_all();
            fleet.snapshots.reconcile(&BTreeMap::new());
            materialized.clear();

            if demand
                .wait_for(|interests| !interests.is_empty())
                .await
                .is_err()
            {
                return;
            }
            revision = *status.borrow_and_update();
            continue;
        }

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
            command_workers.entry(machine).or_insert_with(|| {
                app.state::<Events>().clear_node(machine);
                workers.spawn(CommandWorker::new(fleet.clone(), machine, commands.clone()).run())
            });

            for topic in topics {
                subscriptions
                    .entry((machine, topic.clone()))
                    .or_insert_with(|| {
                        workers.spawn(
                            SubscriptionWorker::new(app.clone(), fleet.clone(), machine, topic)
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
                mesh.borrow_and_update();
                command_workers.clear(); subscriptions.clear(); workers.abort_all();
                app.state::<Events>().registry.lock().unwrap_or_else(PoisonError::into_inner).latest.retain(|(machine, _, _), _| machine == &fleet.me.to_string());
                fleet.snapshots.reconcile(&BTreeMap::new());
                members = fleet.mesh().map(|mesh| mesh.updates());
                beep_node::events::system().changed(Topic::new("projects"));
                beep_node::events::system().changed(Topic::new("accounts"));
            },
            changed = async { match &mut members { Some(members) => members.changed().await, None => std::future::pending().await } } => {
                if changed.is_err() { members = None; }
                beep_node::events::system().changed(Topic::new("projects"));
                beep_node::events::system().changed(Topic::new("accounts"));
            },
            changed = status.changed() => {
                if changed.is_err() { return; }
                let current = *status.borrow_and_update();

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
                Ok(change) if matches!(change.topic.name(), "machines" | "projects" | "home" | "usage" | "accounts" | "sync") => {
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

fn invalidate(app: &AppHandle, topic: &str) {
    if matches!(topic, "agents" | "agent" | "processes" | "*") {
        let outside = app.state::<crate::machines::list::Outside>();
        outside.sessions.stale();
        outside.scans.stale();
        app.state::<Arc<crate::center::home_sessions::AgentSessions>>()
            .stale();
    }

    if matches!(topic, "terminals" | "*") {
        app.state::<crate::machines::list::Outside>()
            .terminals
            .stale();
    }
}

/// Hosts without a push feed are read once for all windows. Results arrive as IPC events.
async fn forge_source(app: AppHandle, mut active: watch::Receiver<bool>) {
    loop {
        if active.wait_for(|active| *active).await.is_err() {
            return;
        }

        let mut previous = None;
        let mut ticks = tokio::time::interval(Duration::from_secs(60));

        loop {
            tokio::select! {
                changed = active.changed() => {
                    if changed.is_err() { return; }
                    if !*active.borrow_and_update() { break; }
                },
                _ = ticks.tick() => {
                    let read = crate::projects::git_items::git_items(
                        app.clone(), app.state::<Arc<beep_fleet::Session>>(),
                        app.state::<Arc<crate::center::view_cache::ViewCache>>());
                    let result = tokio::select! {
                        result = read => result,
                        _ = active.wait_for(|active| !*active) => break,
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
