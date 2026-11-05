//! App-wide commands: link state, this Mac's settings and control, opening
//! links and the log, starting over and quitting.

use crate::{
    beep::{LinkView, SERVER_ENABLED, Service, identity, state::LinkState, state::NoticeVoice},
    center::view_cache::ViewCache,
    error::{DesktopError, Result},
    machines::this_mac::{self, Machine},
    platform::logfile,
    tray,
    voice::LIVE_ENABLED,
};
use beep_proto::node::desktop::Permission;
use serde::Serialize;
use std::{path::Path, process::Command, sync::Arc};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

#[derive(Serialize)]
pub struct Status {
    link: LinkState,
    session: Option<String>,
    /// Signed out of the server in Settings: `link` reads unlinked meanwhile.
    signed_out: bool,
    /// Whether the desktop talks to a server at all (`SERVER_ENABLED`). Off, the window shows
    /// nothing of one: no account, sign in, subscription or assistant.
    server: bool,
    /// Whether live mode can start: the server and `LIVE_ENABLED` are both on. Off, no hint or
    /// shortcut mentions live mode or the orb.
    live: bool,
}

#[tauri::command]
pub fn status(beep: Service<'_>) -> Status {
    Status {
        link: beep.link_state(),
        session: beep.session(),
        signed_out: SERVER_ENABLED && beep.signed_out(),
        server: SERVER_ENABLED,
        live: SERVER_ENABLED && LIVE_ENABLED,
    }
}

#[tauri::command]
pub fn link_offer(beep: Service<'_>) -> Result<LinkView> {
    beep.link_offer(&identity::computer_name())
}

#[tauri::command]
pub async fn unlink(app: AppHandle, beep: Service<'_>) -> Result<()> {
    beep.unlink().await?;
    app.state::<Machine>().sync(&beep);
    // Only the open webviews listen; there is nothing to do if none does.
    let _ = app.emit("link-state", beep.link_state());
    tray::refresh(&app);
    Ok(())
}

/// Forgets this computer's key and starts again with a new one, the only
/// way back after the phone removed it.
#[tauri::command]
pub fn start_over(
    app: AppHandle,
    beep: Service<'_>,
    views: State<'_, Arc<ViewCache>>,
) -> Result<()> {
    beep.chats().clear();
    views.clear();
    identity::forget(&app)?;
    app.restart()
}

#[derive(Serialize)]
pub struct Settings {
    name: String,
    version: String,
    autostart: bool,
    notice_voice: NoticeVoice,
    /// In live mode a pause sends, rather than only "send".
    send_on_pause: bool,
    /// Coding agents notify when they ask or finish a turn.
    agent_notices: bool,
}

#[tauri::command]
pub fn settings(app: AppHandle, beep: Service<'_>) -> Settings {
    Settings {
        name: identity::computer_name(),
        version: app.package_info().version.to_string(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        notice_voice: beep.notice_voice(),
        send_on_pause: beep.send_on_pause(),
        agent_notices: beep.agent_notices(),
    }
}

/// Voice notes: ask first, read aloud right away, or off.
#[tauri::command]
pub fn set_notice_voice(beep: Service<'_>, choice: NoticeVoice) {
    beep.set_notice_voice(choice);
}

/// Whether coding agents notify when they ask or finish a turn.
#[tauri::command]
pub fn set_agent_notices(beep: Service<'_>, enabled: bool) {
    beep.set_agent_notices(enabled);
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<()> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| DesktopError::Other(e.into()))
}

#[tauri::command]
pub fn mac_control(beep: Service<'_>, machine: State<'_, Machine>) -> this_mac::View {
    machine.view(&beep)
}

#[tauri::command]
pub fn set_mac_control(
    beep: Service<'_>,
    machine: State<'_, Machine>,
    enabled: bool,
) -> this_mac::View {
    beep.set_use_mac(enabled);
    machine.sync(&beep);
    machine.view(&beep)
}

#[tauri::command]
pub fn allow_mac_permission(permission: Permission) {
    this_mac::allow(permission);
}

/// The app has no menu bar of its own, so the bar handles ⌘Q.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/// Opens a link from a reply in the browser, never in the bar.
#[tauri::command]
pub fn open_url(url: String) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(DesktopError::BadRequest("only web links open".into()));
    }
    open(&url)
}

/// Shows the app's log folder in Finder, for the window's Show log.
#[tauri::command]
pub fn open_log() -> Result<()> {
    let dir = logfile::dir().ok_or_else(|| DesktopError::BadRequest("no home folder".into()))?;
    open(&dir)
}

/// Hands `target`, a file, folder or link, to macOS's `open`.
pub(crate) fn open(target: impl AsRef<Path>) -> Result<()> {
    Command::new("open")
        .arg(target.as_ref())
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}
