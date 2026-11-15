//! The "Add a machine" sheet: a one-time ticket from this Mac that the
//! machine redeems with `beep-node mesh join <ticket>`. While open, the
//! sheet polls `ticket_machine` to learn which machine joined with it.
//! Only a desktop mints tickets.

use crate::{
    beep::identity::computer_name,
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, files as direct},
};
use beep_mesh::{DeviceKind, JoinTicket, MeshError, PhoneTicket, machine};
use beep_proto::SecretText;
use iroh_tickets::Ticket;
use qrcode::{EcLevel, QrCode, render::svg::Color};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri::{State, ipc::Channel};

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
        Some(qr_svg(&PhoneTicket::from(&ticket))?)
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

/// `code` as a QR code for a phone to scan: in capitals, which a QR code packs smaller, at the
/// lowest error correction, so its modules stay large enough to read off a screen.
fn qr_svg(code: &PhoneTicket) -> Result<String> {
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

/// Has Windows machine `machine_id` run its WSL distribution `distro` as a
/// machine of its own: it installs beep-node there, keeps it running and
/// joins it with a node ticket from this desktop. `on_step` hears each step
/// as it begins.
#[tauri::command]
pub async fn add_wsl_machine(
    fleet: Machines<'_>,
    machine_id: String,
    distro: String,
    on_step: Channel<String>,
) -> Result<()> {
    let client = fleet.client(fleet.machine(&machine_id)?).await?;
    let ticket = fleet.invite(DeviceKind::Node).await?;
    let ticket = SecretText::new(ticket.encode_string());
    let step = |text: String| {
        if let Err(e) = on_step.send(text) {
            log::debug!("the window did not hear a WSL step: {e}");
        }
    };
    machine::add_wsl(&client, &distro, ticket, step)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{e:#}")))
}

/// Has Windows machine `machine_id` stop running the node of its WSL
/// distribution `distro`.
#[tauri::command]
pub async fn stop_wsl_machine(
    fleet: Machines<'_>,
    machine_id: String,
    distro: String,
) -> Result<()> {
    let client = fleet.client(fleet.machine(&machine_id)?).await?;
    machine::stop_wsl(&client, &distro)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{e:#}")))
}

/// Has Windows machine `machine_id` run the node of its WSL distribution
/// `distro` again.
#[tauri::command]
pub async fn start_wsl_machine(
    fleet: Machines<'_>,
    machine_id: String,
    distro: String,
) -> Result<()> {
    let client = fleet.client(fleet.machine(&machine_id)?).await?;
    machine::start_wsl(&client, &distro)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{e:#}")))
}

/// Has Windows machine `machine_id` look into each of its WSL distributions,
/// starting the stopped ones; its next report says what they have.
#[tauri::command]
pub async fn probe_wsl_machines(fleet: Machines<'_>, machine_id: String) -> Result<()> {
    let client = fleet.client(fleet.machine(&machine_id)?).await?;
    machine::probe_wsl(&client)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("{e:#}")))
}
