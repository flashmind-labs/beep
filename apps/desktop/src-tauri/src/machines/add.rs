//! The "Add a machine" sheet: a one-time ticket from this Mac that the
//! machine redeems with `beep-node mesh join <ticket>`. While open, the
//! sheet polls `ticket_machine` to learn which machine joined with it.
//! Only a desktop mints tickets.

use crate::{
    beep::identity::computer_name,
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, files as direct},
};
use beep_mesh::{DeviceKind, JoinTicket, MeshError};
use iroh_tickets::Ticket;
use qrcode::{QrCode, render::svg::Color};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri::State;

/// A ticket as the sheet shows it.
#[derive(Debug, Serialize)]
pub struct TokenView {
    /// The ticket.
    token: String,
    /// What to copy: `beep-node mesh join <ticket>` for a node, the ticket for another Mac.
    command: String,
    /// QR code for a phone ticket.
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
    let token = ticket.encode_string();
    let command = match kind {
        DeviceKind::Node => format!("beep-node mesh join {token}"),
        DeviceKind::Desktop => token.clone(),
    };
    let qr_svg = if phone.unwrap_or(false) {
        Some(qr_svg(&token)?)
    } else {
        None
    };
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
    let ticket = JoinTicket::parse(&ticket).map_err(|e| DesktopError::BadRequest(e.to_string()))?;

    match fleet.join(&ticket, &computer_name()).await {
        Err(DesktopError::Mesh(MeshError::Refused(_))) => {
            Err(DesktopError::BadRequest(USED.into()))
        }
        joined => joined,
    }
}

/// `data` as a QR code for a phone to scan.
fn qr_svg(data: &str) -> Result<String> {
    let qr = QrCode::new(data.as_bytes()).map_err(|e| DesktopError::BadRequest(e.to_string()))?;
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
    let ticket = JoinTicket::parse(&ticket).map_err(|e| DesktopError::BadRequest(e.to_string()))?;
    let joined = fleet.ready().await?.joined_with(&ticket).await?;
    Ok(joined.map(|id| id.to_string()))
}

/// Where `machine_id` looks for git checkouts, `~/` for home.
#[tauri::command]
pub async fn machine_search(fleet: Machines<'_>, machine_id: String) -> Result<String> {
    direct::machine_search(&fleet, fleet.machine(&machine_id)?).await
}

/// Has `machine_id` look for git checkouts in `search` and walk it again.
#[tauri::command]
pub async fn set_machine_search(
    fleet: Machines<'_>,
    machine_id: String,
    search: String,
) -> Result<String> {
    direct::set_machine_search(&fleet, fleet.machine(&machine_id)?, &search).await
}
