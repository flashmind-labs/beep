//! All windows share one duplex topic stream per demanded node.

use super::{Fleet, Machines, Opened};
use crate::error::{DesktopError, Result};
use beep_proto::node::events::{self, CallReply, EventId, Operation, Reply, Request, Topic};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, ipc::Channel};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
    time::{sleep, timeout},
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Interest {
    pub machine: Option<String>,
    pub topic: Topic,
}

#[derive(Clone, Serialize)]
pub struct Update {
    machine_id: String,
    id: EventId,
    topic: Topic,
    resync: bool,
    data: Option<serde_json::Value>,
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

#[derive(Default)]
struct Registry {
    windows: HashMap<String, Window>,
    pending: VecDeque<Pending>,
    latest: BTreeMap<String, EventId>,
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
    commands: Commands,
    manager: Mutex<Option<Job>>,
    source: Mutex<Option<Source>>,
    controller: OnceLock<EndpointId>,
}

impl Default for Events {
    fn default() -> Self {
        Self {
            registry: Mutex::default(),
            demand: watch::channel(Vec::new()).0,
            commands: Commands::default(),
            manager: Mutex::default(),
            source: Mutex::default(),
            controller: OnceLock::new(),
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
        let registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        self.publish(&registry);
    }

    /// Keep publication inside the registry lock so concurrent IPC changes cannot publish stale demand.
    fn publish(&self, registry: &Registry) {
        let topics = registry.interests();
        let forge = topics
            .iter()
            .any(|interest| matches!(interest.topic.name.as_str(), "forge" | "*"));
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
        data: Option<serde_json::Value>,
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
            .iter()
            .filter_map(|(machine_id, cursor)| {
                let update = Update {
                    machine_id: machine_id.clone(),
                    id: *cursor,
                    topic: Topic::new("*"),
                    resync: true,
                    data: self
                        .controller
                        .get()
                        .filter(|local| local.to_string() == *machine_id)
                        .and(data.clone()),
                };
                receives(&held.topics, &update, self.controller.get()).then_some(update)
            })
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

    fn dispatch(&self, update: Update) {
        let destinations: Vec<_> = {
            let mut registry = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            registry.latest.insert(update.machine_id.clone(), update.id);
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
    events.start(app, fleet.inner().clone());
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
        topic.name.as_str(),
        "machines" | "telemetry" | "projects" | "forge" | "home"
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
        source_matches && (update.resync || interest.topic.matches(&update.topic))
    })
}

fn compact_topics(topics: Vec<Topic>) -> Vec<Topic> {
    let all = Topic::new("*");
    if topics.contains(&all) {
        return vec![all];
    }
    let namespaces: BTreeSet<_> = topics
        .iter()
        .filter(|topic| topic.key.is_none())
        .map(|topic| topic.name.as_str())
        .collect();
    topics
        .iter()
        .filter(|topic| topic.key.is_none() || !namespaces.contains(topic.name.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
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
    topics.sort();
    topics.dedup();
    compact_topics(topics)
}

struct Worker {
    topics: watch::Sender<Vec<Topic>>,
    task: tokio::task::AbortHandle,
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
    let mut revision = *status.borrow_and_update();
    let mut workers = JoinSet::new();
    let mut held: BTreeMap<EndpointId, Worker> = BTreeMap::new();
    loop {
        while workers.try_join_next().is_some() {}
        let interests = demand.borrow_and_update().clone();
        if interests.is_empty() {
            held.clear();
            workers.abort_all();
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
        held.retain(|machine, worker| {
            if wanted.contains_key(machine) {
                true
            } else {
                worker.task.abort();
                false
            }
        });
        for (machine, topics) in wanted {
            if let Some(worker) = held.get(&machine) {
                worker.topics.send_if_modified(|current| {
                    if *current == topics {
                        false
                    } else {
                        *current = topics;
                        true
                    }
                });
            } else {
                let (topics, receiver) = watch::channel(topics);
                let (app, fleet, commands) = (app.clone(), fleet.clone(), commands.clone());
                let task = workers
                    .spawn(async move { follow(app, fleet, machine, receiver, commands).await });
                held.insert(machine, Worker { topics, task });
            }
        }
        tokio::select! {
            changed = demand.changed() => if changed.is_err() { return; },
            changed = mesh.changed() => {
                if changed.is_err() { return; }
                mesh.borrow_and_update();
                held.clear(); workers.abort_all();
                app.state::<Events>().registry.lock().unwrap_or_else(PoisonError::into_inner).latest.retain(|machine, _| machine == &fleet.me.to_string());
                members = fleet.mesh().map(|mesh| mesh.updates());
                beep_node::events::system().changed(Topic::new("projects"));
            },
            changed = async { match &mut members { Some(members) => members.changed().await, None => std::future::pending().await } } => {
                if changed.is_err() { members = None; }
                beep_node::events::system().changed(Topic::new("projects"));
            },
            changed = status.changed() => {
                if changed.is_err() { return; }
                let current = *status.borrow_and_update();
                if current.machines != revision.machines { beep_node::events::system().changed(Topic::new("machines")); }
                if current.telemetry != revision.telemetry && interests.iter().any(|interest| matches!(interest.topic.name.as_str(), "telemetry" | "*")) {
                    beep_node::events::system().changed(Topic::new("telemetry"));
                }
                revision = current;
            },
            Some(result) = workers.join_next() => if let Err(error) = result { log::debug!("event reader stopped: {error}"); },
        }
    }
}

async fn follow(
    app: AppHandle,
    fleet: Arc<Fleet>,
    machine: EndpointId,
    mut wanted: watch::Receiver<Vec<Topic>>,
    commands: Commands,
) {
    let mut pause = Duration::from_secs(1);
    let mut resume = Resume::default();
    loop {
        if wanted.has_changed().is_err() {
            return;
        }
        let started = std::time::Instant::now();
        let result = stream(&app, &fleet, machine, &mut wanted, &commands, &mut resume).await;

        if started.elapsed() >= Duration::from_secs(10) {
            pause = Duration::from_secs(1);
        }
        if let Err(error) = result {
            log::debug!("node event stream ended: {error}");
        }
        tokio::select! { _ = sleep(pause) => {}, _ = wanted.wait_for(|_| false) => return }
        pause = (pause * 2).min(Duration::from_secs(30));
    }
}

#[allow(clippy::too_many_arguments)]
async fn stream(
    app: &AppHandle,
    fleet: &Fleet,
    machine: EndpointId,
    wanted: &mut watch::Receiver<Vec<Topic>>,
    commands: &Commands,
    resume: &mut Resume,
) -> Result<()> {
    let client = fleet.client(machine).await?;
    let topics = wanted.borrow_and_update().clone();
    let after = (resume.topics == topics).then_some(resume.cursor).flatten();
    let (mut tx, mut rx) = client
        .open_sending(
            events::PROTOCOL,
            &Request::Watch {
                id: 1,
                topics: topics.clone(),
                after,
            },
        )
        .await?;
    let (orders, mut input) = mpsc::channel::<Call>(16);
    let generation = Uuid::new_v4();
    let _registered = Registered {
        machine,
        commands,
        generation,
    };
    let mut pending = HashMap::new();
    let mut watches = HashMap::from([(1, topics)]);
    let mut next = 1_u64;
    let mut dirty = false;
    let mut reconcile = after.is_some();
    loop {
        if dirty && watches.is_empty() {
            let topics = wanted.borrow_and_update().clone();
            let after = None;
            next = next
                .checked_add(1)
                .ok_or_else(|| DesktopError::BadRequest("request sequence exhausted".into()))?;
            watches.insert(next, topics.clone());
            timeout(
                Duration::from_secs(15),
                tx.send(&Request::Watch {
                    id: next,
                    topics,
                    after,
                }),
            )
            .await
            .map_err(|_| DesktopError::Lost("the machine".into()))??;
            dirty = false;
        }
        tokio::select! {
            changed = wanted.changed() => { if changed.is_err() { return Ok(()); } dirty = true; },
            call = input.recv(), if pending.len() < 16 => if let Some(call) = call {
                next = next.checked_add(1).ok_or_else(|| DesktopError::BadRequest("command sequence exhausted".into()))?;
                timeout(Duration::from_secs(15), tx.send(&Request::Call { id: next, operation: Box::new(call.operation) })).await.map_err(|_| DesktopError::Unsure("the machine".into()))??;
                pending.insert(next, call.reply);
            },
            reply = timeout(Duration::from_secs(35), rx.recv::<Reply>()) => {
                let reply = reply.map_err(|_| DesktopError::Lost("the machine".into()))??.ok_or_else(|| DesktopError::Lost("the machine".into()))?;
                match reply {
                    Reply::Ready => { commands.lock().unwrap_or_else(PoisonError::into_inner).insert(machine, (generation, orders.clone())); },
                    Reply::Watching { id } => if let Some(topics) = watches.remove(&id) {
                        resume.topics = topics;
                        resume.cursor = None;
                    },
                    Reply::Changed(event) => {
                        invalidate(app, &event.topic.name);
                        app.state::<Events>().dispatch(Update { machine_id: machine.to_string(), id: event.id, data: (event.topic.name == "telemetry").then(|| telemetry(&app.state::<Arc<Fleet>>())), topic: event.topic, resync: false });
                        resume.cursor = Some(event.id);
                    },
                    Reply::Resync { cursor: current } => {
                        invalidate(app, "*");
                        app.state::<Events>().dispatch(Update { machine_id: machine.to_string(), id: current, topic: Topic::new("*"), resync: true, data: Some(telemetry(&app.state::<Arc<Fleet>>())) });
                        reconcile = false;
                        resume.cursor = Some(current);
                    },
                    Reply::CaughtUp { cursor: current } => {
                        if !watches.is_empty() { continue; }
                        if reconcile {
                            invalidate(app, "*");
                            app.state::<Events>().dispatch(Update { machine_id: machine.to_string(), id: current, topic: Topic::new("*"), resync: true, data: Some(telemetry(&app.state::<Arc<Fleet>>())) });
                            reconcile = false;
                        }
                        resume.cursor = Some(current);
                    },
                    Reply::Result { id, reply } => if let Some(waiter) = pending.remove(&id) { let _ = waiter.send(Ok(*reply)); },
                    Reply::Error { id, reason } => {
                        if watches.remove(&id).is_some() { return Err(DesktopError::BadRequest(reason)); }
                        if let Some(waiter) = pending.remove(&id) { let _ = waiter.send(Err(reason)); }
                    },
                    _ => {},
                }
            }
        }
    }
}

#[derive(Default)]
struct Resume {
    cursor: Option<EventId>,
    topics: Vec<Topic>,
}

struct Registered<'a> {
    machine: EndpointId,
    commands: &'a Commands,
    generation: Uuid,
}
impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut commands = self.commands.lock().unwrap_or_else(PoisonError::into_inner);
        if commands
            .get(&self.machine)
            .is_some_and(|(id, _)| *id == self.generation)
        {
            commands.remove(&self.machine);
        }
    }
}

fn invalidate(app: &AppHandle, topic: &str) {
    if matches!(topic, "agents" | "processes" | "*") {
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
    let mut previous = None;
    loop {
        if active.wait_for(|active| *active).await.is_err() {
            return;
        }
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
                                beep_node::events::system().changed(Topic::new("forge"));
                                if let Err(error) = app.emit("git-items", items) {
                                    log::debug!("open items listener ended: {error}");
                                }
                            }
                        },
                        Ok(None) => {},
                        Err(error) => log::debug!("open items source: {error}"),
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
mod tests {
    use super::*;

    fn interest(name: &str, machine: Option<EndpointId>) -> Interest {
        Interest {
            machine: machine.map(|id| id.to_string()),
            topic: Topic::new(name),
        }
    }
    fn register(
        events: &Events,
        label: &str,
        id: Uuid,
        topics: Vec<Interest>,
        send: Sink,
    ) -> watch::Receiver<()> {
        events
            .registry
            .lock()
            .unwrap()
            .register(label, id, topics, send)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn windows_share_demand_and_unused_nodes_are_not_dialed() {
        let events = Events::default();
        let local = iroh::SecretKey::generate().public();
        let remote = iroh::SecretKey::generate().public();
        let topics = vec![interest("files", Some(remote))];
        let first = Uuid::new_v4();
        let _one = register(&events, "one", first, topics.clone(), Arc::new(|_| true));
        let _two = register(
            &events,
            "two",
            Uuid::new_v4(),
            topics.clone(),
            Arc::new(|_| true),
        );
        events.changed();
        assert_eq!(*events.demand.borrow(), topics);
        assert_eq!(topics_for(&topics, remote, local), [Topic::new("files")]);
        assert!(topics_for(&topics, local, local).is_empty());
        events.remove("one", first);
        assert_eq!(*events.demand.borrow(), topics);
        events.clear("two");
        assert!(events.demand.borrow().is_empty());
        assert!(topics_for(&[interest("telemetry", None)], remote, local).is_empty());
    }

    #[test]
    fn cancelling_before_registration_prevents_a_late_reader() {
        let mut registry = Registry::default();
        let id = Uuid::new_v4();
        registry.remove("view", id);
        assert!(
            registry
                .register(
                    "view",
                    id,
                    vec![interest("agents", None)],
                    Arc::new(|_| true)
                )
                .unwrap()
                .is_none()
        );
        assert!(registry.interests().is_empty());
    }

    #[test]
    fn early_topic_changes_and_late_cleanup_preserve_the_replacement() {
        let mut registry = Registry::default();
        let id = Uuid::new_v4();
        let topics = vec![interest("reviews", None)];
        registry.topics("view", id, topics.clone()).unwrap();
        let _reader = registry
            .register("view", id, vec![], Arc::new(|_| true))
            .unwrap()
            .unwrap();
        registry.remove("view", Uuid::new_v4());
        assert_eq!(registry.interests(), topics);
    }

    #[test]
    fn dispatch_filters_topics_and_keeps_failed_windows_out_of_demand() {
        let events = Events::default();
        let machine = iroh::SecretKey::generate().public();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let output = heard.clone();
        let _one = register(
            &events,
            "one",
            Uuid::new_v4(),
            vec![interest("reviews", Some(machine))],
            Arc::new(move |event| {
                output.lock().unwrap().push(event);
                true
            }),
        );
        let _two = register(
            &events,
            "two",
            Uuid::new_v4(),
            vec![interest("files", None)],
            Arc::new(|_| false),
        );
        let update = Update {
            machine_id: machine.to_string(),
            topic: Topic::new("reviews"),
            id: EventId {
                epoch: Uuid::new_v4(),
                sequence: 1,
            },
            resync: false,
            data: None,
        };
        events.dispatch(update.clone());
        assert_eq!(heard.lock().unwrap().len(), 1);
        events.dispatch(Update {
            topic: Topic::new("files"),
            ..update
        });
        assert!(!events.registry.lock().unwrap().windows.contains_key("two"));
    }

    #[tokio::test]
    async fn a_lost_command_reply_has_an_unknown_outcome_and_is_not_replayed() {
        let events = Events::default();
        let machine = iroh::SecretKey::generate().public();
        let _reader = register(
            &events,
            "view",
            Uuid::new_v4(),
            vec![interest("agents", None)],
            Arc::new(|_| true),
        );
        let (sender, mut input) = mpsc::channel(1);
        events
            .commands
            .lock()
            .unwrap()
            .insert(machine, (Uuid::new_v4(), sender));
        let receive = async {
            let call = input.recv().await.unwrap();
            drop(call.reply);
        };
        let operation = Operation::Process(beep_proto::node::process::Request::List {});
        let (result, ()) = tokio::join!(events.call("view", machine, operation), receive);
        assert!(matches!(result, Err(DesktopError::Unsure(_))));
        assert!(input.try_recv().is_err());
    }

    #[test]
    fn a_stale_connection_cannot_remove_its_replacement() {
        let machine = iroh::SecretKey::generate().public();
        let (sender, _) = mpsc::channel(1);
        let commands = Commands::default();
        commands
            .lock()
            .unwrap()
            .insert(machine, (Uuid::new_v4(), sender));
        drop(Registered {
            machine,
            commands: &commands,
            generation: Uuid::new_v4(),
        });
        assert!(commands.lock().unwrap().contains_key(&machine));
    }

    fn update(machine: EndpointId, sequence: u64, topic: &str) -> Update {
        Update {
            machine_id: machine.to_string(),
            id: EventId {
                epoch: Uuid::nil(),
                sequence,
            },
            topic: Topic::new(topic),
            resync: false,
            data: None,
        }
    }

    #[test]
    fn an_attaching_window_is_primed_when_shared_demand_does_not_change() {
        let events = Events::default();
        let machine = iroh::SecretKey::generate().public();
        let topics = vec![interest("reviews", Some(machine))];
        let _first = events
            .register(
                "one",
                Uuid::new_v4(),
                topics.clone(),
                Arc::new(|_| true),
                || true,
                None,
            )
            .unwrap()
            .unwrap();
        events.dispatch(update(machine, 7, "reviews"));
        let demand = events.demand.borrow().clone();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let output = heard.clone();
        let _second = events
            .register(
                "two",
                Uuid::new_v4(),
                topics,
                Arc::new(move |event| {
                    output.lock().unwrap().push(event);
                    true
                }),
                || true,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(*events.demand.borrow(), demand);
        events.dispatch(update(machine, 8, "reviews"));
        let heard = heard.lock().unwrap();
        assert_eq!(heard.len(), 2);
        assert!(heard[0].resync);
        assert_eq!(heard[0].id.sequence, 7);
        assert!(!heard[1].resync);
        assert_eq!(heard[1].id.sequence, 8);
    }

    #[test]
    fn a_destroyed_window_cannot_register_or_retain_demand() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let events = Events::default();
        let topics = vec![interest("agents", None)];
        assert!(
            events
                .register(
                    "gone",
                    Uuid::new_v4(),
                    topics.clone(),
                    Arc::new(|_| true),
                    || false,
                    None
                )
                .unwrap()
                .is_none()
        );
        let checks = AtomicUsize::new(0);
        assert!(
            events
                .register(
                    "closing",
                    Uuid::new_v4(),
                    topics,
                    Arc::new(|_| true),
                    || checks.fetch_add(1, Ordering::SeqCst) == 0,
                    None
                )
                .unwrap()
                .is_none()
        );
        assert!(events.registry.lock().unwrap().windows.is_empty());
        assert!(events.demand.borrow().is_empty());
    }

    #[test]
    fn an_overflowing_window_does_not_disrupt_existing_subscriptions() {
        let events = Events::default();
        let machine = iroh::SecretKey::generate().public();
        let mut topics: Vec<_> = (0..events::MAX_TOPICS)
            .map(|number| interest(&format!("plugin_{number}"), None))
            .collect();
        topics.sort();
        let _first = events
            .register(
                "one",
                Uuid::new_v4(),
                topics.clone(),
                Arc::new(|_| true),
                || true,
                None,
            )
            .unwrap()
            .unwrap();
        let overflow = events.register(
            "two",
            Uuid::new_v4(),
            vec![interest("another", Some(machine))],
            Arc::new(|_| true),
            || true,
            None,
        );
        assert!(matches!(overflow, Err(DesktopError::BadRequest(_))));
        assert_eq!(*events.demand.borrow(), topics);
        assert!(!events.registry.lock().unwrap().windows.contains_key("two"));
    }

    #[test]
    fn a_rejected_topic_change_keeps_the_previous_window_topics() {
        let mut registry = Registry::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let topics: Vec<_> = (0..events::MAX_TOPICS)
            .map(|number| interest(&format!("plugin_{number}"), None))
            .collect();
        let _first = registry
            .register("one", first, topics.clone(), Arc::new(|_| true))
            .unwrap()
            .unwrap();
        let _second = registry
            .register("two", second, vec![topics[0].clone()], Arc::new(|_| true))
            .unwrap()
            .unwrap();
        assert!(
            registry
                .topics("two", second, vec![interest("another", None)])
                .is_err()
        );
        assert_eq!(registry.windows["two"].topics, [topics[0].clone()]);
    }

    #[test]
    fn topic_limits_apply_separately_to_distinct_nodes() {
        let mut registry = Registry::default();
        for window in ["one", "two"] {
            let machine = iroh::SecretKey::generate().public();
            let topics = (0..events::MAX_TOPICS)
                .map(|number| interest(&format!("plugin_{number}"), Some(machine)))
                .collect();
            let _reader = registry
                .register(window, Uuid::new_v4(), topics, Arc::new(|_| true))
                .unwrap()
                .unwrap();
        }
        assert_eq!(registry.windows.len(), 2);
    }

    #[test]
    fn broad_namespace_subscriptions_compact_redundant_resource_topics() {
        let events = Events::default();
        let local = iroh::SecretKey::generate().public();
        let topics = vec![interest("reviews", None)];
        let _first = events
            .register(
                "one",
                Uuid::new_v4(),
                topics,
                Arc::new(|_| true),
                || true,
                None,
            )
            .unwrap()
            .unwrap();
        let resources: Vec<_> = (0..events::MAX_TOPICS)
            .map(|number| Interest {
                machine: None,
                topic: Topic::keyed("reviews", format!("review_{number}")),
            })
            .collect();
        let _second = events
            .register(
                "two",
                Uuid::new_v4(),
                resources,
                Arc::new(|_| true),
                || true,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            topics_for(&events.demand.borrow(), local, local),
            [Topic::new("reviews")]
        );
    }

    #[test]
    fn controller_topics_are_unscoped_and_ignore_other_nodes_resyncs() {
        let events = Events::default();
        let local = iroh::SecretKey::generate().public();
        let remote = iroh::SecretKey::generate().public();
        events.controller.set(local).unwrap();
        assert!(validate(&[interest("telemetry", Some(remote))]).is_err());
        assert!(validate(&[interest("telemetry", None)]).is_ok());
        let heard = Arc::new(Mutex::new(Vec::new()));
        let output = heard.clone();
        let _reader = events
            .register(
                "one",
                Uuid::new_v4(),
                vec![interest("telemetry", None)],
                Arc::new(move |event| {
                    output.lock().unwrap().push(event);
                    true
                }),
                || true,
                None,
            )
            .unwrap()
            .unwrap();
        events.dispatch(Update {
            resync: true,
            ..update(remote, 1, "*")
        });
        assert!(heard.lock().unwrap().is_empty());
        events.dispatch(Update {
            resync: true,
            ..update(local, 1, "*")
        });
        assert_eq!(heard.lock().unwrap().len(), 1);
    }

    #[test]
    fn scheduling_workers_does_not_consume_a_pending_mesh_swap() {
        let (source, changed) = watch::channel(Opened::Opening);
        source.send_replace(Opened::Failed("closed".into()));
        assert!(!mesh_is_open(&changed));
        assert!(changed.has_changed().unwrap());
    }
}
