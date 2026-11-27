//! All windows share one duplex stream per subscription on each demanded node.

use super::{
    Fleet, Machines, MeshState, agents as direct,
    outbox::{
        Primed, Reading, Sink, WindowOutbox, compact_topics, controller_topic, home_sources,
        validate,
    },
    session_mirror::SessionMirror,
    terminal_rows,
    view_signal::{Change, Interest, Update, ring},
};
use crate::error::{DesktopError, Result};
use beep_mesh::Snapshots;
use beep_proto::node::events::{self, CallReply, Operation, Topic};
use beep_proto::watched;
use iroh::EndpointId;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
use tokio::{
    sync::{oneshot, watch},
    task::JoinSet,
    time::timeout,
};
use uuid::Uuid;

mod commands;
mod subscriptions;
#[cfg(test)]
use commands::Registered;
use commands::{Call, CommandWorker, Commands};
use subscriptions::SubscriptionWorker;

#[derive(Default)]
pub(crate) struct Events {
    commands: Commands,
    manager: Mutex<Option<Job>>,
}

struct Job(tokio::task::JoinHandle<()>);
impl Drop for Job {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Events {
    pub(crate) fn start(&self, app: AppHandle, fleet: Arc<Fleet>) {
        let outbox = app.state::<WindowOutbox>();
        outbox.start(fleet.me);
        let mut manager = self.manager.lock().unwrap_or_else(PoisonError::into_inner);

        if manager.as_ref().is_none_or(|job| job.0.is_finished()) {
            *manager = Some(Job(tokio::spawn(run(
                app.clone(),
                fleet,
                outbox.demand(),
                self.commands.clone(),
            ))));
        }

        drop(manager);
        outbox.changed();
    }

    pub(crate) async fn call(
        &self,
        outbox: &WindowOutbox,
        window: &str,
        machine: EndpointId,
        operation: Operation,
    ) -> Result<Option<CallReply>> {
        if !outbox.has_window(window) {
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

/// Keeps the nodes pushing the `agents` rows the tray lists while every window is hidden, and
/// starts the streams.
#[cfg(desktop)]
pub(crate) async fn hold_tray(app: AppHandle) {
    let interests = vec![Interest {
        machine: None,
        topic: Topic::new("agents"),
    }];

    if let Err(error) = app.state::<WindowOutbox>().hold_set("tray", interests) {
        log::warn!("the tray cannot follow the sessions: {error}");
    }

    app.state::<Events>()
        .start(app.clone(), app.state::<Arc<Fleet>>().inner().clone());
}

/// Changes a reader's topics, or ends it when `topics` is null. With `paused`, a hidden window
/// stops receiving updates but keeps its streams for a grace period, and showing it again
/// sends only what changed.
#[tauri::command]
pub async fn mesh_topics(
    app: AppHandle,
    window: WebviewWindow,
    events: State<'_, WindowOutbox>,
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
                let grace = events.grace();
                tokio::spawn(async move {
                    tokio::time::sleep(grace).await;
                    app.state::<WindowOutbox>().changed();
                });
            }
        }
        (Some(false), topics) => match events.resume(label, id, topics)? {
            Some(Primed::Full) => ring(&app, Change::Topic(Topic::new("*"))),
            Some(Primed::Changes | Primed::Expired) => ring(&app, Change::Resumed),
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
    events: State<'_, WindowOutbox>,
    id: Uuid,
    topics: Vec<Interest>,
    on_event: Channel<Update>,
    since: Option<u64>,
) -> Result<()> {
    validate(&topics)?;
    let send: Sink = Arc::new(move |update| on_event.send(update).is_ok());
    events.start(fleet.me);
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
    app.state::<Events>()
        .start(app.clone(), fleet.inner().clone());

    // Retained controller views can be evicted while another window stays subscribed.
    if primed == Primed::Full {
        ring(&app, Change::Topic(Topic::new("*")));
    }

    let _ = stopped.changed().await;
    Ok(())
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

fn mesh_is_open(mesh: &watch::Receiver<MeshState>) -> bool {
    watched::read(mesh, |opened| matches!(opened, MeshState::Open(_)))
}

/// Whether the loop has nothing to subscribe to: no view wants a topic and no wait holds one.
fn nothing_wanted(interests: &[Interest], held: bool) -> bool {
    interests.is_empty() && !held
}

/// Resolves when a view wants a topic or a wait holds one.
async fn wake(
    demand: &mut watch::Receiver<Vec<Interest>>,
    holds: &mut watch::Receiver<Vec<(EndpointId, Topic)>>,
) -> std::result::Result<(), watch::error::RecvError> {
    tokio::select! {
        woken = watched::wait_for(demand, |interests| !interests.is_empty()) => woken,
        woken = watched::wait_for(holds, |holds| !holds.is_empty()) => woken,
    }
}

/// Resolves when what views want or what waits hold changes.
async fn demand_or_hold_changed(
    demand: &mut watch::Receiver<Vec<Interest>>,
    holds: &mut watch::Receiver<Vec<(EndpointId, Topic)>>,
) -> std::result::Result<(), watch::error::RecvError> {
    tokio::select! {
        changed = demand.changed() => changed,
        changed = holds.changed() => changed,
    }
}

/// The topics to subscribe to on each machine: what views want plus what waits hold.
fn wanted_topics(
    snapshots: &Snapshots,
    interests: &[Interest],
    machines: &[EndpointId],
    me: EndpointId,
) -> BTreeMap<EndpointId, Vec<Topic>> {
    let mut wanted: BTreeMap<_, _> = machines
        .iter()
        .filter_map(|&machine| {
            let topics = topics_for(interests, machine, me);
            (!topics.is_empty()).then_some((machine, topics))
        })
        .collect();
    snapshots.add_holds(&mut wanted, machines);

    wanted
}

async fn run(
    app: AppHandle,
    fleet: Arc<Fleet>,
    mut demand: watch::Receiver<Vec<Interest>>,
    commands: Commands,
) {
    let mut mesh = fleet.reach.opened();
    let mut members = fleet.mesh().map(|mesh| mesh.updates());
    let mut status = fleet.status.changes();
    let mut holds = fleet.reach.snapshots().holds();
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
        let held = watched::read_and_update(&mut holds, |holds| !holds.is_empty());

        if nothing_wanted(&interests, held) {
            command_workers.clear();
            subscriptions.clear();
            workers.abort_all();
            fleet.reach.snapshots().reconcile(&BTreeMap::new());
            materialized.clear();

            if wake(&mut demand, &mut holds).await.is_err() {
                return;
            }
            revision = watched::read_and_update(&mut status, |status| *status);
            continue;
        }

        // A window follows a terminal by its id, its node knows it by number.
        let interests =
            terminal_rows::node_interests(interests, &terminal_rows::places(&fleet), |machine| {
                terminal_rows::old_node(&fleet, machine)
            });
        let mut machines = if mesh_is_open(&mesh) {
            fleet.online()
        } else {
            Vec::new()
        };
        machines.push(fleet.me);
        machines.sort();
        machines.dedup();
        let wanted = wanted_topics(fleet.reach.snapshots(), &interests, &machines, fleet.me);
        fleet.reach.snapshots().reconcile(&wanted);
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
            if machine == fleet.me && !fleet.reach.hosts() {
                continue;
            }

            command_workers.entry(machine).or_insert_with(|| {
                app.state::<WindowOutbox>().clear_node(machine);
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
            ring(&app, Change::Topic(Topic::new("*")));
            app.state::<WindowOutbox>().publish_views(
                fleet.me,
                Topic::new("telemetry"),
                telemetry(&fleet),
            );
            materialized = interests.clone();
        }

        tokio::select! {
            changed = demand_or_hold_changed(&mut demand, &mut holds) => if changed.is_err() { return; },
            changed = mesh.changed() => {
                if changed.is_err() { return; }
                watched::read_and_update(&mut mesh, |_| ());
                command_workers.clear(); subscriptions.clear(); workers.abort_all();
                app.state::<WindowOutbox>().keep_only(fleet.me);
                fleet.reach.snapshots().reconcile(&BTreeMap::new());
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
                    app.state::<WindowOutbox>().publish_views(fleet.me, change.topic, telemetry(&fleet));
                },
                // This node's sync status also comes as a snapshot of its own sync stream, which tells
                // a transfer's progress from its end.
                Ok(change) if change.topic.name() == "sync" => {
                    ring(&app, Change::Sync { moving: true });
                },
                Ok(change) if matches!(change.topic.name(), "machines" | "tools" | "projects" | "home" | "usage" | "accounts") => {
                    ring(&app, Change::Topic(change.topic));
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    ring(&app, Change::Topic(Topic::new("*")));
                    app.state::<WindowOutbox>().publish_views(fleet.me, Topic::new("telemetry"), telemetry(&fleet));
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
