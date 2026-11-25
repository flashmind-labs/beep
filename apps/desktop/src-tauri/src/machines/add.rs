//! The "Add a machine" sheet: a one-time ticket from this Mac that the
//! machine redeems with `beep-node mesh join <ticket>`. The machines view
//! carries `ticket_machine` for each ticket minted and not yet expired, so
//! the sheet learns which machine joined with it from the push.
//! Only a desktop mints tickets.

use crate::{
    beep::identity::computer_name,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        event_views::View,
        files as direct,
        node_actor::{FAST_JOB_WAIT, JobKind},
    },
};
use beep_mesh::{DeviceKind, Invitation, MeshError, ShortTicket};
use iroh_tickets::Ticket;
use qrcode::{EcLevel, QrCode, render::svg::Color};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, State};

/// The most tickets followed at once: the newest.
const TICKETS_KEPT: usize = 16;

/// The tickets this desktop minted that have not expired, with when each does.
#[derive(Default)]
pub(crate) struct Tickets(Mutex<Vec<(String, u64)>>);

impl Tickets {
    fn keep(&self, token: String, expires_at: u64) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.push((token, expires_at));

        if held.len() > TICKETS_KEPT {
            held.remove(0);
        }
    }

    /// The tickets that have not expired.
    fn live(&self) -> Vec<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|(_, expires_at)| *expires_at > now);
        held.iter().map(|(token, _)| token.clone()).collect()
    }
}

/// `ticket_machine` for each ticket minted and not expired, for the machines view to push.
pub(crate) async fn ticket_views(app: &AppHandle) -> Vec<View> {
    let fleet = app.state::<Arc<Fleet>>();
    let reads = app
        .state::<Tickets>()
        .live()
        .into_iter()
        .map(|ticket| async move {
            let args = serde_json::json!({ "ticket": ticket });

            match joined_by(&fleet, &ticket).await {
                Ok(machine) => View::new("ticket_machine", args, machine),
                Err(error) => Some(View::failed("ticket_machine", args, &error.to_string())),
            }
        });

    futures::future::join_all(reads)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// A ticket as the sheet shows it.
#[derive(Debug, Serialize)]
pub struct TokenView {
    /// The ticket.
    token: String,
    /// What to copy: `beep-node mesh join <ticket>` for a node, the ticket for another Mac.
    command: String,
    /// QR code of the ticket, for a phone.
    qr_svg: Option<String>,
    /// In milliseconds since the Unix epoch.
    expires_at: u64,
}

/// A ticket that brings a device of `kind` into the mesh, with what to copy: the line a node
/// runs, or the ticket itself for another Mac to paste. A `temporary` node, such as a
/// container, is removed after a day offline.
#[tauri::command]
pub async fn mesh_invite(
    fleet: State<'_, Arc<Fleet>>,
    tickets: State<'_, Tickets>,
    kind: DeviceKind,
    temporary: Option<bool>,
    phone: Option<bool>,
) -> Result<TokenView> {
    let ticket = if phone.unwrap_or(false) {
        if kind != DeviceKind::Desktop || temporary.unwrap_or(false) {
            return Err(DesktopError::BadRequest(
                "A phone pairs as a workspace controller.".into(),
            ));
        }

        fleet
            .ready()
            .await?
            .invite_observer(Duration::from_secs(300))
            .await?
    } else {
        match (kind, temporary.unwrap_or(false)) {
            (DeviceKind::Node, true) => fleet.invite_temporary().await?,
            _ => fleet.invite(kind).await?,
        }
    };
    let code = ShortTicket::from(&ticket);
    let token = code.encode_string();
    let command = match kind {
        DeviceKind::Node => format!("beep-node mesh join {token}"),
        DeviceKind::Desktop => token.clone(),
    };
    let qr_svg = if phone.unwrap_or(false) {
        Some(qr_svg(&code)?)
    } else {
        None
    };
    tickets.keep(token.clone(), ticket.expires_ms);
    Ok(TokenView {
        token,
        qr_svg,
        command,
        expires_at: ticket.expires_ms,
    })
}

/// What the join window says of a ticket that does not work.
const USED: &str = "This ticket expired or was already used. Ask for a new one.";

/// Joins the mesh a ticket from another desktop names, under this Mac's name. A Mac that is
/// alone in its own mesh leaves it and brings its settings along.
#[tauri::command]
pub async fn mesh_join(fleet: State<'_, Arc<Fleet>>, ticket: String) -> Result<()> {
    let invite = ticket
        .parse::<Invitation>()
        .map_err(|e| DesktopError::BadRequest(e.to_string()))?;

    match fleet.join(invite, &computer_name()).await {
        Err(DesktopError::Mesh(MeshError::Refused(_))) => {
            Err(DesktopError::BadRequest(USED.into()))
        }
        joined => joined,
    }
}

/// `code` as a QR code for a phone to scan: in capitals, which a QR code packs smaller, at the
/// lowest error correction, so its modules stay large enough to read off a screen.
fn qr_svg(code: &ShortTicket) -> Result<String> {
    let data = code.encode_string().to_ascii_uppercase();
    let qr = QrCode::with_error_correction_level(data, EcLevel::L)
        .map_err(|e| DesktopError::BadRequest(e.to_string()))?;
    Ok(qr
        .render::<Color>()
        .quiet_zone(false)
        .dark_color(Color("#0A0E0C"))
        .light_color(Color("#F4F8F5"))
        .build())
}

/// The id of the machine that joined with `ticket`, `None` while it is
/// unused. The desktop records the authenticated key that redeemed the secret.
#[tauri::command]
pub async fn ticket_machine(
    fleet: State<'_, Arc<Fleet>>,
    ticket: String,
) -> Result<Option<String>> {
    joined_by(&fleet, &ticket).await
}

async fn joined_by(fleet: &Fleet, ticket: &str) -> Result<Option<String>> {
    let invite = ticket
        .parse::<Invitation>()
        .map_err(|e| DesktopError::BadRequest(e.to_string()))?;
    let joined = fleet.ready().await?.joined_with(&invite).await?;
    Ok(joined.map(|id| id.to_string()))
}

/// Where `machine_id` looks for git checkouts, `~/` for home.
#[tauri::command]
pub async fn machine_search(fleet: Machines<'_>, machine_id: String) -> Result<String> {
    let machine = fleet.machine(&machine_id)?;
    let fleet_arc = fleet.inner().clone();
    fleet
        .projections()
        .search
        .read(
            machine,
            PartialEq::eq,
            || {},
            move || async move { direct::machine_search(&fleet_arc, machine).await },
        )
        .await
}

/// Has `machine_id` look for git checkouts in `search` and walk it again.
#[tauri::command]
pub async fn set_machine_search(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    search: String,
) -> Result<String> {
    let machine = fleet.machine(&machine_id)?;
    let trimmed = search.trim();
    let optimistic = if trimmed.is_empty() {
        "~/".to_owned()
    } else {
        trimmed.to_owned()
    };
    fleet.projections().search.put(machine, optimistic.clone());
    let fleet_arc = fleet.inner().clone();
    fleet
        .node_ops()
        .submit(
            Some(app),
            fleet_arc,
            machine,
            JobKind::SetSearch { search },
            FAST_JOB_WAIT,
        )
        .await?;
    Ok(fleet
        .projections()
        .search
        .peek(&machine)
        .unwrap_or(optimistic))
}
