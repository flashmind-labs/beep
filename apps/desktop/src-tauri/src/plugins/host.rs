use std::ffi::OsString;
use std::path::{Path, PathBuf};

use beep_plugins::{Host, HostError, Placement};
use tauri::{AppHandle, Emitter};

/// Names the folder plugins load from, overriding the app data folder's.
const DIR_ENV: &str = "BEEP_PLUGINS_DIR";
/// Sent to every window with all placements each time they change.
const CHANGED: &str = "plugins-changed";

/// Starts the host on the plugins folder. The app data folder holds it unless
/// `BEEP_PLUGINS_DIR` names another.
pub(crate) fn start(app: &AppHandle, data: &Path) -> Result<Host, HostError> {
    let dir = plugins_dir(std::env::var_os(DIR_ENV), data);
    let emitter = app.clone();

    Host::start(dir, move |placements: &[Placement]| {
        if let Err(e) = emitter.emit(CHANGED, placements) {
            log::debug!("the page missed a plugin update: {e}");
        }
    })
}

fn plugins_dir(configured: Option<OsString>, data: &Path) -> PathBuf {
    match configured {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => data.join("plugins"),
    }
}

#[cfg(test)]
#[path = "../tests/plugins_host.rs"]
mod tests;
