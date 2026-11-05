//! The devices: the desktops of the mesh, which any of them adds and removes,
//! and the phones the server links. Starting an account here, joining
//! another by its code and showing a code a phone joins with are the
//! server's.

use crate::{
    beep::state::LinkState,
    beep::{Beep, Service, qr_svg},
    error::{DesktopError, Result},
    fleet::Fleet,
};
use beep_mesh::DeviceKind;
use beep_proto::{
    DeviceInvite, InviteDevice, JoinAccount, JoinPreview, LinkedDevice, ListDevices, PeekInvite,
    RemoveDevice,
    link::{format_join_code, join_code},
};
use iroh::EndpointId;
use serde::Serialize;
use std::{sync::Arc, time::SystemTime};
use tauri::{AppHandle, State};

/// A join code for a phone to scan or type.
#[derive(Serialize)]
pub struct PhoneInvite {
    code: String,
    qr_svg: String,
    expires_in: u64,
}

/// Starts an account on this Mac instead of linking it to a phone. The
/// server makes it on the first connection.
#[tauri::command]
pub async fn use_here(app: AppHandle, beep: Service<'_>) -> Result<()> {
    start_here(&app, &beep).await
}

async fn start_here(app: &AppHandle, beep: &Beep) -> Result<()> {
    // A Mac signed out has an account: signing in is the same key again.
    if beep.signed_out() {
        return super::signout::resume(app, beep).await;
    }
    if beep.link_state() != LinkState::Unlinked {
        return Ok(());
    }
    beep.set_link_state(LinkState::Linked);
    if let Err(e) = beep.call(ListDevices {}).await {
        beep.set_link_state(LinkState::Unlinked);
        return Err(e);
    }
    crate::tray::became_linked(app);
    Ok(())
}

/// A device on the account, as Settings lists it.
#[derive(Serialize)]
pub struct DeviceView {
    id: String,
    name: String,
    /// `phone` or `desktop`.
    kind: &'static str,
    /// A desktop, which adds and removes other devices.
    primary: bool,
    this_device: bool,
}

impl From<LinkedDevice> for DeviceView {
    fn from(device: LinkedDevice) -> Self {
        Self {
            id: device.id,
            name: device.name,
            kind: device.kind.as_str(),
            primary: device.primary,
            this_device: device.this_device,
        }
    }
}

/// Whether the server answered a list of its devices.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ServerReach {
    /// There is no server, or this Mac is signed out of it.
    None,
    Answered,
    Unreachable,
}

/// The devices Settings lists, and whether the server they came from answered.
#[derive(Serialize)]
pub struct Devices {
    devices: Vec<DeviceView>,
    server: ServerReach,
}

/// The desktops of the mesh, then the phones the server links, when there
/// is a server.
#[tauri::command]
pub async fn list_devices(beep: Service<'_>, fleet: State<'_, Arc<Fleet>>) -> Result<Devices> {
    let mut devices: Vec<DeviceView> = fleet
        .machines()
        .into_iter()
        .map(|status| status.device)
        .filter(|device| device.kind == DeviceKind::Desktop)
        .map(|device| DeviceView {
            this_device: device.id == fleet.id(),
            id: device.id.to_string(),
            name: device.name,
            kind: "desktop",
            primary: true,
        })
        .collect();
    let mut server = ServerReach::None;
    if beep.server().is_some() {
        match beep.call(ListDevices {}).await {
            Ok(linked) => {
                server = ServerReach::Answered;
                devices.extend(
                    linked
                        .into_iter()
                        .filter(|device| device.kind.as_str() == "phone")
                        .map(DeviceView::from),
                );
            }
            Err(e) => {
                server = ServerReach::Unreachable;
                log::debug!("the server's devices are not listed: {e}");
            }
        }
    }
    Ok(Devices { devices, server })
}

/// A code that moves the phone that scans it, with its chats, into this
/// Mac's account.
#[tauri::command]
pub async fn invite_phone(beep: Service<'_>) -> Result<PhoneInvite> {
    let name = crate::beep::identity::computer_name();
    let DeviceInvite {
        code,
        uri,
        expires_at,
    } = beep.call(InviteDevice { name }).await?;
    let expires_in = expires_at
        .duration_since(SystemTime::now())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(PhoneInvite {
        code: format_join_code(&code),
        qr_svg: qr_svg(&uri)?,
        expires_in,
    })
}

/// Removes a desktop from the mesh, or unlinks a phone from the server.
#[tauri::command]
pub async fn remove_device(
    beep: Service<'_>,
    fleet: State<'_, Arc<Fleet>>,
    id: String,
) -> Result<()> {
    if let Ok(device) = id.parse::<EndpointId>()
        && fleet.machines().iter().any(|m| m.device.id == device)
    {
        return fleet.remove(device).await;
    }
    beep.call(RemoveDevice { id }).await
}

/// Moves this Mac's account into the one that showed `code`, starting one
/// here first when it has none. Returns the name of the device that showed
/// the code.
#[tauri::command]
pub async fn join_account(app: AppHandle, beep: Service<'_>, code: String) -> Result<String> {
    let code = join_code(&code)
        .ok_or_else(|| DesktopError::BadRequest("that is not a join code".into()))?;
    start_here(&app, &beep).await?;
    let JoinPreview { name, .. } = beep.call(PeekInvite { code: code.clone() }).await?;
    beep.call(JoinAccount { code }).await?;
    // The server closes this connection; the next call signs in again, now
    // into the account joined, and the copy of the chats starts over.
    crate::tray::became_linked(&app);
    Ok(name)
}
