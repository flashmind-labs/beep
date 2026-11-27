//! What coding agents on the machines of the mesh want the user for, sent
//! by the machines themselves. The alerts and runs streams are
//! `beep_fleet::alerts`; this adds the native CLI sessions and the phones
//! each machine tells, and routes all of it to the app. An answer goes back
//! to the machine.

use super::{Fleet, MeshState};
use crate::error::Result;
use beep_fleet::alerts::{self as shared, Heard as Told};
use beep_mesh::{MeshError, RETRY};
use beep_proto::watched;
use beep_proto::{
    AgentNoticeKey, Notice,
    node::agent::{ExternalSession, RunNews, SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
    node::device::{self, Device, DeviceId},
};
use iroh::EndpointId;
use std::{
    collections::HashSet,
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
    /// A question or a turn that ended; `allow_deny` when Allow and Deny
    /// answer the question.
    Notice {
        notice: Box<Notice>,
        allow_deny: bool,
    },
    /// A CI run the user started ended, told by the machine that watches
    /// its project.
    Run(Box<RunNews>),
    /// A native CLI opened, updated or closed a session on this machine.
    Presence {
        machine: EndpointId,
        sessions: Vec<ExternalSession>,
    },
    /// adb on `machine` sees a phone it did not list before.
    Phone {
        machine: EndpointId,
        device: Box<Device>,
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
    let mut opened = fleet.reach.opened();

    loop {
        let current = watched::read_and_update(&mut opened, Clone::clone);
        let MeshState::Open(mesh) = current else {
            // No mesh to follow: the streams let go of it, so it can close.
            if opened.changed().await.is_err() {
                return;
            }
            continue;
        };
        let (alerts, mut told) = shared::follow(fleet.reach().clone(), mesh.clone());
        let presence = shared::each_machine(&mesh, |id| {
            tokio::spawn(presence(fleet.clone(), id, out.clone()))
        });
        let phones = shared::each_machine(&mesh, |id| {
            tokio::spawn(phones(fleet.clone(), id, out.clone()))
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
            _ = async { select! { _ = alerts => {}, _ = presence => {}, _ = phones => {}, _ = routing => {} } } => {
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
        Told::Notice {
            notice, options, ..
        } => Direct::Notice {
            notice,
            allow_deny: crate::platform::notify::allow_deny(&options),
        },
        Told::Run(news) => Direct::Run(news),
    }
}

/// Follows `id`'s native CLI sessions, again after each break, until the
/// receiver goes.
#[tracing::instrument(skip_all, fields(peer = %id.fmt_short()))]
async fn presence(fleet: Arc<Fleet>, id: EndpointId, out: mpsc::Sender<Direct>) {
    let mut retry = Retry::new("session presence");

    loop {
        let mut replied = false;
        let followed = stream(&fleet, id, &out, &mut replied).await;

        if !retry.again(&fleet, id, followed, replied).await {
            return;
        }
    }
}

/// Follows `id`'s phones, again after each break, until the receiver goes,
/// telling the app of each new one. The phones of the first list were
/// there before, so they tell nothing.
#[tracing::instrument(skip_all, fields(peer = %id.fmt_short()))]
async fn phones(fleet: Arc<Fleet>, id: EndpointId, out: mpsc::Sender<Direct>) {
    let mut retry = Retry::new("phones");
    let mut known = None;

    loop {
        let mut replied = false;
        let followed = phone_stream(&fleet, id, &out, &mut known, &mut replied).await;

        if !retry.again(&fleet, id, followed, replied).await {
            return;
        }
    }
}

/// When a machine's stream is opened again after it broke: `RETRY` after
/// the machine is online.
struct Retry {
    what: &'static str,
    /// An offline machine fails the same way on every try: logged once,
    /// and again when the error changes or after a stream that worked.
    logged: Option<String>,
    started: Instant,
}

impl Retry {
    fn new(what: &'static str) -> Self {
        Self {
            what,
            logged: None,
            started: Instant::now(),
        }
    }

    /// Waits to follow `id` again after a stream ended with `followed`;
    /// false once it ended with `Ok`, when the receiver is gone. `replied`
    /// says whether the machine sent anything.
    async fn again(
        &mut self,
        fleet: &Fleet,
        id: EndpointId,
        followed: Result<()>,
        replied: bool,
    ) -> bool {
        let Err(e) = followed else {
            return false;
        };
        let error = e.to_string();

        if self.logged.as_ref() != Some(&error) {
            log::debug!(
                "following {} from {}: {error}",
                self.what,
                fleet.name_of(id)
            );
            self.logged = Some(error);
        }

        if held(replied, self.started.elapsed()) {
            self.logged = None;
        }

        until_online(fleet, id).await;
        sleep(RETRY).await;
        self.started = Instant::now();

        true
    }
}

/// Waits until `id` is online, at once if it is, so an offline machine is
/// not asked again until it comes back.
/// Returns once `id` is online, or the mesh closed.
#[tracing::instrument(skip_all, fields(peer = %id.fmt_short()))]
pub(crate) async fn until_online(fleet: &Fleet, id: EndpointId) {
    let mut changes = fleet.changes();

    while !fleet.online().contains(&id) {
        if !changes.changed().await {
            return;
        }
    }
}

/// Whether a stream that broke was a working one, so its next error is
/// logged again: the machine sent something, or kept it open for `HELD`.
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

/// One stream of `id`'s phones. `Ok` when the receiver is gone; `known`
/// holds the phones last listed, `None` before the first list.
async fn phone_stream(
    fleet: &Fleet,
    id: EndpointId,
    out: &mpsc::Sender<Direct>,
    known: &mut Option<HashSet<DeviceId>>,
    replied: &mut bool,
) -> Result<()> {
    let client = fleet.client(id).await?;
    let (mut tx, mut rx) = client.open(device::PROTOCOL).await?;
    tx.send(&device::Request::FollowPhones {})
        .await
        .map_err(MeshError::Link)?;

    // `tx` stays open: finishing it ends the stream.
    loop {
        let reply = rx.recv::<device::Reply>().await.map_err(MeshError::Link)?;
        let Some(device::Reply::Devices { devices }) = reply else {
            let reason = "the machine does not follow its phones";
            return Err(MeshError::Link(anyhow::anyhow!(reason)).into());
        };
        *replied = true;

        for device in new_phones(known, devices) {
            let phone = Direct::Phone {
                machine: id,
                device: Box::new(device),
            };

            if out.send(phone).await.is_err() {
                return Ok(());
            }
        }
    }
}

/// The phones of `listed` that `known` does not hold, none for the first
/// list; `known` becomes `listed`.
fn new_phones(known: &mut Option<HashSet<DeviceId>>, listed: Vec<Device>) -> Vec<Device> {
    let ids = listed.iter().map(|device| device.id.clone()).collect();
    let fresh = match known.as_ref() {
        Some(known) => listed
            .into_iter()
            .filter(|device| !known.contains(&device.id))
            .collect(),
        None => Vec::new(),
    };
    *known = Some(ids);

    fresh
}

/// Answers the question `key` names, on the machine that asked it. Refused
/// as a conflict when the session no longer asks it.
pub(crate) async fn answer(fleet: &Fleet, key: &AgentNoticeKey, reply: &str) -> Result<()> {
    Ok(shared::answer(fleet.reach(), key, reply).await?)
}

/// Whether `machine`, as text, is this Mac or a member of its mesh: the
/// machines `answer` reaches.
pub(crate) fn reaches(fleet: &Fleet, machine: &str) -> bool {
    EndpointId::from_str(machine)
        .is_ok_and(|id| id == fleet.id() || fleet.mesh().is_some_and(|mesh| mesh.is_member(id)))
}

#[cfg(all(test, desktop))]
#[path = "../tests/fleet_alerts.rs"]
mod tests;
