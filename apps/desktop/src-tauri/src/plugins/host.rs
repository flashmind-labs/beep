use std::ffi::OsString;
use std::path::{Path, PathBuf};

use beep_plugins::{Builtin, Handler, Host, HostError, Placement, PluginId, Policy, Topic};
use tauri::{AppHandle, Emitter};

/// Names the folder plugins load from, overriding the app data folder's.
const DIR_ENV: &str = "BEEP_PLUGINS_DIR";
/// Sent to every window with all placements each time they change.
const CHANGED: &str = "plugins-changed";
/// Sent to every window with the topics plugins follow each time they change.
const WANTED: &str = "plugins-wanted";
/// Sent to every window when a plugin is added, removed or changes state.
const STATUS: &str = "plugins-status";

/// Starts the host on the plugins folder. The app data folder holds it unless
/// `BEEP_PLUGINS_DIR` names another.
pub(crate) fn start(app: &AppHandle, data: &Path) -> Result<Host, HostError> {
    let configured = std::env::var_os(DIR_ENV);
    let policy = policy(configured.as_deref());
    let dir = plugins_dir(configured, data);
    super::install::clean_stale(&dir);

    Host::start(dir, Windows { app: app.clone() }, policy, &builtins())
}

/// The composer cat's files, which are embedded in the app and shipped as a
/// built-in plugin. `plugins/examples/composer-cat` is the source.
const COMPOSER_CAT: &[(&str, &[u8])] = &[
    (
        "beep-plugin.json",
        include_bytes!("../../../../../plugins/examples/composer-cat/beep-plugin.json"),
    ),
    (
        "main.ts",
        include_bytes!("../../../../../plugins/examples/composer-cat/main.ts"),
    ),
    (
        "art/cat-blink.png",
        include_bytes!("../../../../../plugins/examples/composer-cat/art/cat-blink.png"),
    ),
    (
        "art/cat-ear.png",
        include_bytes!("../../../../../plugins/examples/composer-cat/art/cat-ear.png"),
    ),
    (
        "art/cat-happy.png",
        include_bytes!("../../../../../plugins/examples/composer-cat/art/cat-happy.png"),
    ),
    (
        "art/cat-idle.png",
        include_bytes!("../../../../../plugins/examples/composer-cat/art/cat-idle.png"),
    ),
    (
        "art/cat-tail-a.png",
        include_bytes!("../../../../../plugins/examples/composer-cat/art/cat-tail-a.png"),
    ),
    (
        "art/cat-tail-b.png",
        include_bytes!("../../../../../plugins/examples/composer-cat/art/cat-tail-b.png"),
    ),
];

/// The plugins that ship with the app. The id is checked by a test against the
/// embedded manifest.
fn builtins() -> Vec<Builtin> {
    let cat = "composer-cat".parse::<PluginId>().ok();

    cat.into_iter()
        .map(|id| Builtin {
            id,
            files: COMPOSER_CAT,
        })
        .collect()
}

/// Tells the windows what the host reports.
struct Windows {
    app: AppHandle,
}

impl Handler for Windows {
    fn shown(&self, placements: &[Placement]) {
        if let Err(e) = self.app.emit(CHANGED, placements) {
            log::debug!("the page missed a plugin update: {e}");
        }
    }

    fn wanted(&self, topics: &[Topic]) {
        if let Err(e) = self.app.emit(WANTED, topics) {
            log::debug!("the page missed a plugin topic change: {e}");
        }
    }

    fn status_changed(&self) {
        if let Err(e) = self.app.emit(STATUS, ()) {
            log::debug!("the page missed a plugin status change: {e}");
        }
    }
}

/// A folder named by `BEEP_PLUGINS_DIR` is for development: its plugins run without
/// an approval record. The app data folder's plugins wait for the user's approval.
fn policy(configured: Option<&std::ffi::OsStr>) -> Policy {
    match configured {
        Some(dir) if !dir.is_empty() => Policy::Dev,
        _ => Policy::Managed,
    }
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
