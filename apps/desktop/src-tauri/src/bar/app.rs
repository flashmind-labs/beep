//! App-wide commands: this Mac's settings and control, opening links and
//! the log, and quitting.

use crate::{
    beep::{identity, state::Prefs},
    error::{DesktopError, Result},
    fleet::Machines,
    machines::this_mac::{self, Machine},
    platform::logfile,
};
use beep_proto::node::desktop::Permission;
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, State};
#[cfg(desktop)]
use tauri_plugin_autostart::ManagerExt;

#[derive(Serialize)]
pub struct Settings {
    name: String,
    version: String,
    autostart: bool,
    /// Coding agents notify when they ask or finish a turn.
    agent_notices: bool,
}

#[tauri::command]
pub fn settings(app: AppHandle, prefs: State<'_, Prefs>) -> Settings {
    Settings {
        name: identity::computer_name(),
        version: app.package_info().version.to_string(),
        #[cfg(desktop)]
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        #[cfg(mobile)]
        autostart: false,
        agent_notices: prefs.agent_notices(),
    }
}

/// Whether coding agents notify when they ask or finish a turn.
#[tauri::command]
pub fn set_agent_notices(prefs: State<'_, Prefs>, enabled: bool) {
    prefs.set_agent_notices(enabled);
}

#[cfg(mobile)]
#[tauri::command]
pub fn set_autostart(_enabled: bool) -> Result<()> {
    Err(DesktopError::BadRequest(
        "A phone has no login item.".into(),
    ))
}

#[cfg(desktop)]
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
pub fn mac_control(
    prefs: State<'_, Prefs>,
    machine: State<'_, Machine>,
    fleet: Machines<'_>,
) -> this_mac::View {
    machine.view(&prefs, fleet.id())
}

#[tauri::command]
pub fn set_mac_control(
    prefs: State<'_, Prefs>,
    machine: State<'_, Machine>,
    fleet: Machines<'_>,
    enabled: bool,
) -> this_mac::View {
    prefs.set_use_mac(enabled);
    machine.sync(&prefs);
    machine.view(&prefs, fleet.id())
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

/// Shows the app's log folder in Finder, for Show log in the tray and on Home.
#[tauri::command]
pub fn open_log() -> Result<()> {
    let dir = logfile::dir().ok_or_else(|| DesktopError::BadRequest("no home folder".into()))?;
    open(&dir)
}

/// Hands `target`, a file, folder or link, to macOS's `open`.
pub(crate) fn open(target: impl AsRef<Path>) -> Result<()> {
    crate::platform::open(target.as_ref())?;
    Ok(())
}
