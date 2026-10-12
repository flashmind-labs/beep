//! This Mac's own account, and the devices on the account: starting one
//! here, joining another by its code, and showing a code a phone joins with.

use crate::{
    beep::{Beep, qr_svg},
    error::{DesktopError, Result},
    state::LinkState,
};
use beep_proto::{
    DeviceInvite, InviteDevice, JoinAccount, JoinPreview, LinkedDevice, ListDevices, PeekInvite,
    RemoveDevice,
    link::{format_join_code, join_code},
};
use serde::Serialize;
use std::{sync::Arc, time::SystemTime};
use tauri::{AppHandle, State};

type Service<'a> = State<'a, Arc<Beep>>;

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
    if beep.link_state() != LinkState::Unlinked {
        return Ok(());
    }
    beep.set_link_state(LinkState::Linked);
    if let Err(e) = beep.call(ListDevices {}).await {
        beep.set_link_state(LinkState::Unlinked);
        return Err(e);
    }
    crate::became_linked(app);
    Ok(())
}

/// A device on the account, as Settings lists it.
#[derive(Serialize)]
pub struct DeviceView {
    id: String,
    name: String,
    /// `phone` or `desktop`.
    kind: &'static str,
    /// The device that made the account, which links and removes others.
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

#[tauri::command]
pub async fn list_devices(beep: Service<'_>) -> Result<Vec<DeviceView>> {
    let devices = beep.call(ListDevices {}).await?;
    Ok(devices.into_iter().map(DeviceView::from).collect())
}

/// A code that moves the phone that scans it, with its chats, into this
/// Mac's account.
#[tauri::command]
pub async fn invite_phone(beep: Service<'_>) -> Result<PhoneInvite> {
    let name = crate::computer_name();
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

#[tauri::command]
pub async fn remove_device(beep: Service<'_>, id: String) -> Result<()> {
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
    crate::became_linked(&app);
    Ok(name)
}
