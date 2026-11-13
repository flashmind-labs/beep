//! What coding agents on the machines of the mesh want the user for, sent
//! by the machines themselves. The alerts and runs streams are
//! `beep_fleet::alerts`; this adds the native CLI sessions each machine
//! tells, and routes all of it to the app. An answer goes back to the
//! machine.

use super::{Fleet, Opened};
use crate::error::Result;
use beep_fleet::alerts::{self as shared, Heard as Told};
use beep_mesh::MeshError;
use beep_node::link::reconnect::Backoff;
use beep_proto::{
    AgentNoticeKey, Notice,
    node::agent::{ExternalSession, RunNews, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
};
use iroh::EndpointId;
use std::{
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{select, sync::mpsc, time::sleep};

/// Notices and connections waiting for the app to route them.
const QUEUE: usize = 64;
/// How long a stream stays open before it counts as working.
const HELD: Duration = Duration::from_secs(30);

/// What the streams tell the app.
pub(crate) enum Direct {
    /// A stream to a machine opened: what the app shows from machines may
    /// have changed while it was closed.
    Connected,
    Notice(Box<Notice>),
    /// A CI run the user started ended, told by the machine that watches
    /// its project.
    Run(Box<RunNews>),
    /// A native CLI opened, updated or closed a session on this machine.
    Presence {
        machine: EndpointId,
        sessions: Vec<ExternalSession>,
    },
}

/// What follows every machine of the mesh, now and as it changes, and the
/// receiver for what it hears. The future runs until the receiver is
/// dropped; the caller spawns it.
pub(crate) fn start(fleet: Arc<Fleet>) -> (impl Future<Output = ()>, mpsc::Receiver<Direct>) {
    let (out, heard) = mpsc::channel(QUEUE);
    (follow(fleet, out), heard)
}

async fn follow(fleet: Arc<Fleet>, out: mpsc::Sender<Direct>) {
    let mut opened = fleet.mesh.subscribe();

    loop {
        let current = opened.borrow_and_update().clone();
        let Opened::Open(mesh) = current else {
            // No mesh to follow: the streams let go of it, so it can close.
            if opened.changed().await.is_err() {
                return;
            }
            continue;
        };
        let (alerts, mut told) = shared::follow(fleet.clone(), mesh.clone());
        let presence = shared::each_machine(&mesh, |id| {
            tokio::spawn(presence(fleet.clone(), id, out.clone()))
        });
        let routing = async {
            while let Some(told) = told.recv().await {
                if out.send(direct(told)).await.is_err() {
                    return;
                }
            }
        };
        select! {
            _ = out.closed() => return,
            // Joined another mesh: look again.
            changed = opened.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = async { select! { _ = alerts => {}, _ = presence => {}, _ = routing => {} } } => {
                // The mesh closed: wait for the next one.
                if opened.changed().await.is_err() {
                    return;
                }
            }
        }
    }
}

fn direct(told: Told) -> Direct {
    match told {
        Told::Connected => Direct::Connected,
        Told::Notice { notice, .. } => Direct::Notice(notice),
        Told::Run(news) => Direct::Run(news),
    }
}

/// Follows `id`'s native CLI sessions, again after each break, until the
/// receiver goes.
async fn presence(fleet: Arc<Fleet>, id: EndpointId, out: mpsc::Sender<Direct>) {
    let mut backoff = Backoff::eager();
    // An offline machine fails the same way on every try: logged once,
    // and again when the error changes or after a stream that worked.
    let mut logged = None;

    loop {
        let started = Instant::now();
        let mut replied = false;

        match stream(&fleet, id, &out, &mut replied).await {
            Ok(()) => return,
            Err(e) => {
                let error = e.to_string();

                if logged.as_ref() != Some(&error) {
                    log::debug!(
                        "following session presence from {}: {error}",
                        fleet.name_of(id)
                    );
                    logged = Some(error);
                }
            }
        }

        if held(replied, started.elapsed()) {
            backoff.reset();
            logged = None;
        }

        until_online(&fleet, id).await;
        sleep(backoff.pause()).await;
    }
}

/// Waits until `id` is online, at once if it is, so an offline machine is
/// not asked again until it comes back.
/// Returns once `id` is online, or the mesh closed.
pub(crate) async fn until_online(fleet: &Fleet, id: EndpointId) {
    let mut changes = fleet.changes();

    while !fleet.online().contains(&id) {
        if !changes.changed().await {
            return;
        }
    }
}

/// Whether a stream that broke was a working one, so the next try comes at
/// once: the machine sent something, or kept it open for `HELD`. One that
/// is accepted and closed again keeps backing off.
fn held(replied: bool, lived: Duration) -> bool {
    replied || lived >= HELD
}

/// One stream's life. `Ok` when the receiver is gone; `replied` says
/// whether the machine sent anything.
async fn stream(
    fleet: &Fleet,
    id: EndpointId,
    out: &mpsc::Sender<Direct>,
    replied: &mut bool,
) -> Result<()> {
    let client = fleet.client(id).await?;
    let (mut tx, mut rx) = client.open(SESSIONS_PROTOCOL).await?;
    tx.send(&SessionsRequest::FollowPresence {})
        .await
        .map_err(MeshError::Link)?;
    // `tx` stays open: finishing it ends the stream.
    loop {
        let reply = rx.recv::<SessionsReply>().await.map_err(MeshError::Link)?;
        let Some(reply) = reply else {
            return Err(MeshError::Link(anyhow::anyhow!("the machine ended the stream")).into());
        };
        *replied = true;

        if let SessionsReply::External { sessions } = reply
            && out
                .send(Direct::Presence {
                    machine: id,
                    sessions,
                })
                .await
                .is_err()
        {
            return Ok(());
        }
    }
}

/// Answers the question `key` names, on the machine that asked it. Refused
/// as a conflict when the session no longer asks it.
pub(crate) async fn answer(fleet: &Fleet, key: &AgentNoticeKey, reply: &str) -> Result<()> {
    Ok(shared::answer(fleet, key, reply).await?)
}

/// Whether `machine`, as text, is this Mac or a member of its mesh: the
/// machines `answer` reaches.
pub(crate) fn reaches(fleet: &Fleet, machine: &str) -> bool {
    EndpointId::from_str(machine)
        .is_ok_and(|id| id == fleet.id() || fleet.mesh().is_some_and(|mesh| mesh.is_member(id)))
}

#[cfg(test)]
#[path = "../tests/fleet_alerts.rs"]
mod tests;
