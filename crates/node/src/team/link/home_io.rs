//! Keeping the team link current with the mesh, and handing what waits to a desktop.

use super::TeamLink;
use crate::{
    agents::desktop_call::forward as to_desktop,
    team::home::{on_team, read},
};
use beep_mesh::{DeviceKind, Key, Mesh, TeamWrites, keep_contacts, take_roster};
use beep_proto::{
    SessionId, lenient,
    node::tools::{Reply, Request, TeamRosterArgs, Tool},
    team::TeamRecord,
};
use futures::future::{join_all, pending};
use serde::Serialize;
use std::{mem, time::{Duration, SystemTime}};
use tokio::{
    join,
    select,
    sync::broadcast::error::RecvError,
    task::JoinSet,
    time::{Instant, sleep, sleep_until},
};

/// How often what waits for a desktop is offered again.
const RETRY: Duration = Duration::from_secs(60);
/// Keeps `link` current with `mesh` and hands what waits to a desktop, until
/// the mesh is gone. The handing runs as a task of its own, one at a time, so
/// a desktop that answers slowly never holds up a refresh; what changed while
/// it ran is handed on its next run.
pub(crate) async fn follow(mesh: Mesh, link: TeamLink) {
    let mut changes = mesh.changes();
    let mut handing: JoinSet<Handed> = JoinSet::new();
    let mut handed = Vec::new();
    let mut again = true;
    // Set while a roster or an invite is still waiting for a desktop.
    let mut retry_at: Option<Instant> = None;
    refresh(&mesh, &link);

    loop {
        if again && handing.is_empty() {
            again = false;
            handing.spawn(hand(mesh.clone(), link.clone(), mem::take(&mut handed)));
        }

        let due = retry_at;
        let retry = async {
            match due {
                Some(at) => sleep_until(at).await,
                None => pending().await,
            }
        };

        let expiry = async {
            match link.next_expiry() {
                Some(at) => sleep(at.duration_since(SystemTime::now()).unwrap_or_default()).await,
                None => pending().await,
            }
        };

        select! {
            // An invite or a card that outlives its day goes, and its topic says so.
            () = expiry => link.expire_now(),
            () = link.expiry_changed() => {}
            change = changes.recv() => match change {
                Ok(Key::Team(_) | Key::Contact(_) | Key::Project(_)) | Err(RecvError::Lagged(_)) => {
                    refresh(&mesh, &link);
                    again = true;
                }
                Ok(_) => {}
                Err(RecvError::Closed) => return,
            },
            () = link.waiting() => again = true,
            () = retry => {
                retry_at = None;
                again = true;
            }
            Some(done) = handing.join_next() => {
                // A task that panicked hands nothing over; the next run starts again.
                let done = done.unwrap_or_default();
                handed = done.handed;
                retry_at = done.waiting.then(|| Instant::now() + RETRY);
            }
        }
    }
}

/// What a run of `hand` leaves for the next.
#[derive(Default)]
struct Handed {
    /// The rosters a desktop took, so they are not sent again.
    handed: Vec<TeamRecord>,
    /// Whether something still waits for a desktop.
    waiting: bool,
}

/// Whether this device is a desktop, which writes the mesh's state.
fn desktop_here(mesh: &Mesh) -> bool {
    mesh.devices()
        .iter()
        .any(|status| status.device.id == mesh.me() && status.device.kind == DeviceKind::Desktop)
}

fn refresh(mesh: &Mesh, link: &TeamLink) {
    let records = mesh
        .scan("team/")
        .into_iter()
        .filter_map(|(key, bytes)| match key {
            Key::Team(repo) => Some((repo, lenient::from_slice::<TeamRecord>(&bytes).ok()?)),
            _ => None,
        })
        .filter(|(_, record)| on_team(record, mesh))
        .collect();
    let home = read(mesh);

    link.with(|teams| teams.rosters.refresh(records));
    link.set_home(home);
}


/// Keeps the contacts in step with the teams, then hands the rosters and
/// invites that wait to a desktop. A desktop writes its own rosters; a roster
/// already in `handed` is not sent again. The calls to the desktop run
/// together.
async fn hand(mesh: Mesh, link: TeamLink, mut handed: Vec<TeamRecord>) -> Handed {
    let desktop_here = desktop_here(&mesh);

    // A desktop's node writes rosters itself, so its desktop keeps the
    // contacts in step with them here.
    if desktop_here
        && let Err(e) = keep_contacts(&mesh, &TeamWrites::lock().await).await
    {
        log::warn!("the contacts were not kept in step with the teams: {e}");
    }

    let invites = link.invites_waiting();
    let records = link.with(|teams| teams.rosters.pending());
    handed.retain(|record| records.contains(record));
    let unhanded: Vec<_> = records
        .into_iter()
        .filter(|record| !handed.contains(record))
        .collect();

    let rosters = join_all(unhanded.into_iter().map(|record| async {
        let written = match desktop_here {
            true => take_roster(&mesh, record.clone(), &TeamWrites::lock().await)
                .await
                .map(drop)
                .map_err(|e| e.to_string()),
            false => {
                call(
                    &mesh,
                    Tool::TeamRoster,
                    &TeamRosterArgs {
                        record: record.clone(),
                    },
                )
                .await
            }
        };

        (record, written)
    }));

    // A desktop's own node keeps the invites for the desktop.
    let invites = join_all(invites.into_iter().filter(|_| !desktop_here).map(|invite| async {
        let sent = call(&mesh, Tool::TeamInvite, &invite).await;

        (invite, sent)
    }));

    let (rosters, invites) = join!(rosters, invites);
    let mut waiting = false;

    for (record, written) in rosters {
        match written {
            Ok(()) => handed.push(record),
            Err(e) => {
                waiting = true;
                log::info!("the roster of {} waits for a desktop: {e}", record.repo);
            }
        }
    }

    for (invite, sent) in invites {
        match sent {
            Ok(()) => link.drop_invite(&invite),
            Err(e) => {
                waiting = true;
                log::info!("an invite to {} waits for a desktop: {e}", invite.repo);
            }
        }
    }

    Handed { handed, waiting }
}

/// Calls `tool` with `args` on a desktop of the mesh.
async fn call(mesh: &Mesh, tool: Tool, args: &impl Serialize) -> Result<(), String> {
    let request = Request {
        session: SessionId::default(),
        tool,
        args: serde_json::to_string(args).map_err(|e| e.to_string())?,
        approved: Default::default(),
    };

    match to_desktop(mesh, &request).await {
        Reply::Done { failed: false, .. } => Ok(()),
        Reply::Done { text, .. } => Err(text),
        _ => Err("the desktop answered something else".to_owned()),
    }
}

