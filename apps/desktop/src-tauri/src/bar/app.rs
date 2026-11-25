//! App-wide commands: this Mac's settings and control, opening links and
//! the log, and quitting.

use crate::{
    beep::{identity, state::Prefs},
    error::{DesktopError, Result},
    fleet::Machines,
    machines::{
        ThisMacView,
        this_mac::{self, Machine},
    },
    platform::logfile,
};
use beep_proto::node::desktop::Permission;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, State};
#[cfg(desktop)]
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize)]
pub struct BuildCapabilities {
    local_host: bool,
    metrics_recording: bool,
    autostart: bool,
    image_clipboard: bool,
    desktop_control: bool,
}

#[tauri::command]
pub fn build_capabilities() -> BuildCapabilities {
    BuildCapabilities {
        local_host: cfg!(desktop),
        metrics_recording: true,
        autostart: cfg!(desktop),
        image_clipboard: cfg!(desktop),
        desktop_control: cfg!(target_os = "macos"),
    }
}

#[derive(Serialize)]
pub struct Settings {
    name: String,
    version: String,
    autostart: bool,
    /// Coding agents notify when they ask or finish a turn.
    agent_notices: bool,
    /// Where saves go now.
    save_folder: Option<PathBuf>,
    /// Whether that folder was picked rather than Downloads.
    save_folder_picked: bool,
    /// Updates to every release, patches included.
    developer: bool,
    /// Machines running an older beep-node update themselves when seen online.
    node_auto_update: bool,
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
        save_folder: crate::platform::saves_dir(&app).ok(),
        save_folder_picked: prefs.save_folder().is_some(),
        developer: prefs.developer(),
        node_auto_update: prefs.node_auto_update(),
    }
}

/// Saves go to `folder`, one picked on this computer.
#[tauri::command]
pub fn set_save_folder(prefs: State<'_, Prefs>, folder: PathBuf) -> Result<()> {
    if !folder.is_absolute() || !folder.is_dir() {
        return Err(DesktopError::BadRequest(format!(
            "{} is not a folder on this computer.",
            folder.display()
        )));
    }

    prefs.set_save_folder(Some(folder));
    Ok(())
}

/// Saves go to Downloads again.
#[tauri::command]
pub fn use_downloads(prefs: State<'_, Prefs>) {
    prefs.set_save_folder(None);
}

/// Whether coding agents notify when they ask or finish a turn.
#[tauri::command]
pub fn set_agent_notices(prefs: State<'_, Prefs>, enabled: bool) {
    prefs.set_agent_notices(enabled);
}

/// Whether machines running an older beep-node update themselves when seen online.
#[tauri::command]
pub fn set_node_auto_update(prefs: State<'_, Prefs>, enabled: bool) {
    prefs.set_node_auto_update(enabled);
}

/// Developer mode: updates to every release instead of minor releases only.
#[tauri::command]
pub fn set_developer_mode(app: AppHandle, prefs: State<'_, Prefs>, enabled: bool) {
    prefs.set_developer(enabled);
    crate::platform::update::follow_feed(&app);
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
) -> ThisMacView {
    machine.view(&prefs, fleet.id())
}

#[tauri::command]
pub fn set_mac_control(
    prefs: State<'_, Prefs>,
    machine: State<'_, Machine>,
    fleet: Machines<'_>,
    enabled: bool,
) -> Result<ThisMacView> {
    if !build_capabilities().local_host {
        return Err(DesktopError::BadRequest(
            "A phone does not host local work.".into(),
        ));
    }

    prefs.set_use_mac(enabled);
    machine.sync(&prefs);
    Ok(machine.view(&prefs, fleet.id()))
}

#[tauri::command]
pub fn allow_mac_permission(permission: Permission) -> Result<()> {
    if !build_capabilities().desktop_control {
        return Err(DesktopError::BadRequest(
            "Desktop control is only available on macOS.".into(),
        ));
    }

    this_mac::allow(permission);
    Ok(())
}

/// The app has no menu bar of its own, so the bar handles ⌘Q.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/// Opens a link from a reply in the browser, never in the bar. The opener
/// plugin also opens it on a phone, where the `open` crate cannot.
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(DesktopError::BadRequest("only web links open".into()));
    }

    app.opener()
        .open_url(url, None::<&str>)
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// Shows the app's log folder in Finder, for Show log in the tray and on Home.
#[tauri::command]
pub fn open_log() -> Result<()> {
    let dir = logfile::dir().ok_or_else(|| DesktopError::BadRequest("no home folder".into()))?;
    crate::platform::open(&dir)
}
