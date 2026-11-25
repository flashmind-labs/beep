//! One snapshot worker per demanded topic, independent of action delivery.

use super::{Events, Fleet, Snapshots, Update, commands::retrying, controller_topic, invalidate};
use crate::error::{DesktopError, Result};
use crate::fleet::{
    session_mirror::SessionMirror,
    view_signal::{Change, moving, ring},
};
use beep_proto::node::{
    agent::SessionsReply,
    events::{self, Topic},
    subscriptions::{Reply, Request},
};
use iroh::EndpointId;
use std::{
    collections::HashSet,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokio::time::{Sleep, sleep, timeout};
use uuid::Uuid;

/// A silent stream is lost after this long without a message.
const IDLE: Duration = Duration::from_secs(35);

/// The deadline for the next message. It lives across loop turns and
/// restarts only when a message arrives.
struct Idle(Pin<Box<Sleep>>);

impl Idle {
    fn new() -> Self {
        Self(Box::pin(sleep(IDLE)))
    }

    fn reset(&mut self) {
        self.0.as_mut().reset(tokio::time::Instant::now() + IDLE);
    }

    async fn expired(&mut self) {
        self.0.as_mut().await;
    }
}

pub(super) struct SubscriptionWorker {
    pub(super) app: AppHandle,
    pub(super) fleet: Arc<Fleet>,
    pub(super) machine: EndpointId,
    pub(super) topic: Topic,
}

impl SubscriptionWorker {
    pub(super) async fn run(self) {
        retrying(
            || self.connect(),
            |error, elapsed_ms| {
                log::warn!(
                    "subscription stream ended machine={} topic={} elapsed_ms={elapsed_ms}: {error}",
                    self.machine,
                    self.topic
                );
            },
        )
        .await;
    }

    async fn connect(&self) -> Result<()> {
        let Self {
            app,
            fleet,
            machine,
            topic,
        } = self;
        let machine = *machine;
        let client = fleet.client(machine).await?;
        let generation = Uuid::new_v4();
        let (mut tx, mut rx) = client.open_subscriptions().await?;
        if !fleet
            .snapshots
            .connected(machine, topic.clone(), generation)
        {
            return Ok(());
        }
        let _snapshots = SnapshotConnection {
            app,
            topic,
            snapshots: &fleet.snapshots,
            machine,
            generation,
        };
        let mut query_changes = fleet.snapshots.changes();
        let mut refresh = app.state::<Events>().refresh.subscribe();
        let queries = fleet.snapshots.queries_for(machine, topic);
        app.state::<Events>()
            .forget_queries(machine, &fleet.snapshots.queries(machine));
        let mut sent = queries.clone();
        log::debug!(
            "node watch sending machine={machine} generation={generation} watch=1 topics=1 queries={}",
            queries.len()
        );
        timeout(
            Duration::from_secs(15),
            tx.send(&Request::Watch {
                id: 1,
                topic: topic.clone(),
                queries,
                deltas: true,
            }),
        )
        .await
        .map_err(|_| DesktopError::Lost("the machine".into()))??;
        let mut watches = HashSet::from([1]);
        let mut sweep = tokio::time::interval(super::snapshots::LINGER / 3);
        let mut next = 1_u64;
        let mut dirty = false;
        let mut revision = 0;
        let mut idle = Idle::new();
        // The node sends the `agents` list whole once, then the rows that changed.
        let mut listed = events::HeldSessions::default();

        loop {
            if dirty && watches.is_empty() {
                let queries = fleet.snapshots.queries_for(machine, topic);
                app.state::<Events>()
                    .forget_queries(machine, &fleet.snapshots.queries(machine));

                if queries == sent {
                    dirty = false;
                    continue;
                }

                sent = queries.clone();
                next = next
                    .checked_add(1)
                    .ok_or_else(|| DesktopError::BadRequest("request sequence exhausted".into()))?;
                log::debug!(
                    "node watch sending machine={machine} generation={generation} watch={next} topics=1 queries={}",
                    queries.len()
                );
                watches.insert(next);
                timeout(
                    Duration::from_secs(15),
                    tx.send(&Request::Watch {
                        id: next,
                        topic: topic.clone(),
                        queries,
                        deltas: true,
                    }),
                )
                .await
                .map_err(|_| DesktopError::Lost("the machine".into()))??;
                dirty = false;
            }

            tokio::select! {
                _ = sweep.tick() => {
                    fleet.snapshots.sweep(machine, topic, Instant::now());
                },
                changed = query_changes.changed() => {
                    if changed.is_err() { return Ok(()); }
                    dirty = true;
                },
                changed = refresh.changed() => {
                    if changed.is_err() { return Ok(()); }
                    next = next.checked_add(1).ok_or_else(|| DesktopError::BadRequest("request sequence exhausted".into()))?;
                    watches.insert(next);
                    timeout(Duration::from_secs(15), tx.send( &Request::Refresh { id: next }))
                        .await.map_err(|_| DesktopError::Lost("the machine".into()))??;
                },
                () = idle.expired() => {
                    return Err(DesktopError::Lost("the machine".into()));
                },
                reply = rx.recv::<Reply>() => {
                    let reply = reply?.ok_or_else(|| DesktopError::Lost("the machine".into()))?;
                    idle.reset();

                    match reply {
                        Reply::Watching { id } => {
                            log::debug!("node watch acknowledged machine={machine} generation={generation} watch={id}");
                            watches.remove(&id);
                        },
                        Reply::Snapshot(snapshot) => {
                            // A delta that does not follow the last message means one was missed:
                            // watching again starts with the whole list.
                            let Some(snapshot) = listed.hear(*snapshot) else {
                                return Err(DesktopError::Lost("the machine".into()));
                            };
                            log::debug!("node snapshot received machine={machine} generation={generation} query={:?} topic={} revision={}", snapshot.query, snapshot.topic.name(), snapshot.revision);
                            revision = snapshot.revision;
                            let Some(initial) = fleet.snapshots.install(machine, generation, &snapshot) else { continue; };

                            if snapshot.topic.name() == super::TOOL_UPDATES {
                                tool_updates_heard(app, fleet, machine, Some(&snapshot.value));
                            }

                            team_heard(app, machine, &snapshot.topic, Some(&snapshot.value));

                            if snapshot.topic.name() == "terminals" {
                                fleet.projections().terminal_pushes.heard(machine, Some(&snapshot.value));
                            }

                            crate::notices::heard(app, machine, snapshot.topic.name(), &snapshot.value);

                            if let events::Value::Resources(resources) = &snapshot.value {
                                app.state::<Events>().resources(machine, &snapshot.topic, resources);
                            }

                            // The first agents list of a subscription is pushed to the mirror too: nothing reads it back.
                            if snapshot.query.is_some() && snapshot.topic.name() == "agents" {
                                // A read someone follows, not the machine's own list: a scan of it hears the change.
                                if !initial && matches!(&snapshot.value, events::Value::Sessions(reply) if matches!(reply.as_ref(), SessionsReply::External { .. })) {
                                    app.state::<Arc<SessionMirror>>().rescan(machine);
                                }
                            } else if !initial || snapshot.topic.name() == "agents" { invalidate(app, machine, &snapshot.topic, Some(&snapshot.value)); }
                            if !initial && snapshot.topic.name() == "sync" {
                                ring(app, Change::Sync { moving: moving(&snapshot.value) });
                            } else if !initial && snapshot.topic.name() != "agents" && (machine == fleet.me || matches!(snapshot.topic.name(), "agent" | "processes" | "terminals" | "moves")) {
                                // "agents" draws when `invalidate` changes the mirror's rows, which Home follows.
                                ring(app, Change::Sessions(snapshot.topic.clone()));
                            }

                            // A node from before the topics of one screen says only that a list changed.
                            if !initial && matches!(snapshot.topic.name(), "terminals" | "processes") {
                                ring(app, Change::TerminalsChanged { machine, topic: snapshot.topic.clone() });
                            }

                            if matches!(snapshot.topic.name(), "terminal-output" | "process-output") && snapshot.topic.key().is_some() {
                                // The windows get the screen as the view the command gives.
                                ring(app, Change::OutputPushed { machine, topic: snapshot.topic.clone(), value: snapshot.value.clone() });
                            } else if (machine != fleet.me || !controller_topic(&snapshot.topic))
                                && app.state::<Events>().node_wanted(machine, &snapshot.topic) {
                                let data = serde_json::to_value(&snapshot.value).ok();
                                let update = Update { machine_id: machine.to_string(), revision: snapshot.revision, query: snapshot.query, initial, data, topic: snapshot.topic, seq: 0 };

                                // A folder's followed reads go out as views in its place, so its readers have nothing to read again.
                                if update.topic.name() != "files" || !crate::agents::files_live::heard(app, machine, &update) {
                                    app.state::<Events>().dispatch_node(update);
                                }
                            }
                        },
                        Reply::SnapshotError { query, topic, reason } => {
                            log::warn!("node snapshot failed machine={machine} generation={generation} query={query:?} topic={}: {reason}", topic.name());
                            if let Some(query) = query && !fleet.snapshots.failed(machine, generation, query, reason.clone()) { continue; }

                            if topic.name() == super::TOOL_UPDATES {
                                tool_updates_heard(app, fleet, machine, None);
                            }
                            invalidate(app, machine, &topic, None);
                            team_heard(app, machine, &topic, None);

                            if topic.name() == "terminals" {
                                fleet.projections().terminal_pushes.heard(machine, None);
                            }

                            if machine == fleet.me || matches!(topic.name(), "agents" | "agent" | "processes" | "terminals" | "moves" | "sync") {
                                ring(app, Change::Failed { topic: topic.clone(), reason: reason.clone() });
                            }

                            if (machine != fleet.me || !controller_topic(&topic)) && app.state::<Events>().node_wanted(machine, &topic) {
                                app.state::<Events>().dispatch_node(Update { machine_id: machine.to_string(), revision, query, initial: false, data: Some(serde_json::json!({ "error": reason })), topic, seq: 0 });
                            }
                        },
                        Reply::Error { id, reason } if watches.remove(&id) => {
                            return Err(DesktopError::BadRequest(reason));
                        },
                        _ => {},
                    }
                }
            }
        }
    }
}

/// A node answered on a team topic: the team pages hear it.
fn team_heard(app: &AppHandle, machine: EndpointId, topic: &Topic, value: Option<&events::Value>) {
    if matches!(
        topic.name(),
        events::TEAM_CARDS_TOPIC | events::TEAM_INVITES_TOPIC
    ) {
        ring(
            app,
            Change::Team {
                machine,
                topic: topic.clone(),
                value: value.cloned(),
            },
        );
    }
}

/// A node answered on its `tool-updates` topic, with its list or with an error as one that
/// predates the topic does: the tools view is drawn again when that changes who is asked.
fn tool_updates_heard(
    app: &AppHandle,
    fleet: &Fleet,
    machine: EndpointId,
    answer: Option<&events::Value>,
) {
    if fleet.projections().tool_pushes.heard(machine, answer) {
        ring(app, Change::Topic(Topic::new("tools")));
    }
}

struct SnapshotConnection<'a> {
    app: &'a AppHandle,
    topic: &'a Topic,
    snapshots: &'a Snapshots,
    machine: EndpointId,
    generation: Uuid,
}

impl Drop for SnapshotConnection<'_> {
    fn drop(&mut self) {
        if self.topic.name() == "terminals" {
            self.app
                .state::<Arc<Fleet>>()
                .projections()
                .terminal_pushes
                .forget(self.machine);
        }

        self.snapshots.end(self.machine, self.generation);

        // Its rows are no longer pushed: the mirror lists the machine again when they age.
        if self.topic.name() == "agents" {
            self.app
                .state::<Arc<SessionMirror>>()
                .unpushed(self.machine);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/fleet_events_subscriptions.rs"]
mod tests;
