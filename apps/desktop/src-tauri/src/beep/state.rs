//! What the app remembers between launches, in `state.json`, and `Prefs`,
//! which holds it for the windows.

use beep_proto::node::update::ReleaseChannel;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};
use tauri::LogicalSize;

/// The page zoom of every window, in percent, from 70 to 200.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Zoom(u16);

impl Zoom {
    const MIN: u16 = 70;
    const MAX: u16 = 200;

    /// `percent`, brought into range.
    pub fn new(percent: u16) -> Self {
        Self(percent.clamp(Self::MIN, Self::MAX))
    }

    /// In range even when a hand-edited file says otherwise.
    pub fn percent(self) -> u16 {
        self.0.clamp(Self::MIN, Self::MAX)
    }

    /// The webview's zoom factor, 1.0 at 100%.
    pub fn factor(self) -> f64 {
        f64::from(self.percent()) / 100.0
    }
}

impl Default for Zoom {
    fn default() -> Self {
        Self(100)
    }
}

/// The saved state. A file from an older app may hold more; what is not
/// read here is dropped on the next save.
#[derive(Debug, Serialize, Deserialize)]
pub struct Saved {
    /// The "Let Beep use this Mac" switch from before `mac_on`, still
    /// written so an older app reads a matching value. It was off unless
    /// turned on, not a choice, so it no longer decides.
    #[serde(default)]
    pub use_mac: bool,
    /// "Let Beep use this Mac" as this app version saved it. Missing in
    /// files from before it, which start with the Mac on once, as a fresh
    /// install does.
    #[serde(default)]
    pub mac_on: Option<bool>,
    /// Coding agents notify when they ask something or finish a turn.
    #[serde(default = "on")]
    pub agent_notices: bool,
    /// Machines running an older beep-node update themselves when seen online.
    #[serde(default = "on")]
    pub node_auto_update: bool,
    /// Page zoom of every window, ⌘= ⌘- ⌘0.
    #[serde(default)]
    pub zoom: Zoom,
    /// The folder saves go to, picked in Settings; Downloads without one.
    #[serde(default)]
    pub save_folder: Option<PathBuf>,
    /// Developer mode: updates to every release, patches included, instead
    /// of minor releases only.
    #[serde(default)]
    pub developer: bool,
    /// The size of the last project window, which a project opened in a
    /// window for the first time takes.
    #[serde(default)]
    pub project_window: Option<LogicalSize<f64>>,
}

fn on() -> bool {
    true
}

impl Saved {
    /// "Let Beep use this Mac": on unless turned off in this version.
    #[cfg(desktop)]
    pub fn uses_mac(&self) -> bool {
        self.mac_on.unwrap_or(true)
    }

    /// Brings a file from an older app up to date. Its `use_mac` false was
    /// the old default rather than a choice, so the Mac starts on.
    fn migrate(&mut self) {
        if self.mac_on.is_none() {
            self.set_use_mac(true);
        }
    }

    pub fn set_use_mac(&mut self, on: bool) {
        self.mac_on = Some(on);
        self.use_mac = on;
    }
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            use_mac: true,
            mac_on: Some(true),
            agent_notices: true,
            node_auto_update: true,
            zoom: Zoom::default(),
            save_folder: None,
            developer: false,
            project_window: None,
        }
    }
}

pub struct StateFile(PathBuf);

impl StateFile {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn load(&self) -> Saved {
        let mut saved: Saved = fs::read(&self.0)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        saved.migrate();
        saved
    }

    pub fn save(&self, saved: &Saved) {
        let result = serde_json::to_vec_pretty(saved)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| write(&self.0, &bytes));
        if let Err(e) = result {
            log::warn!("saving {}: {e:#}", self.0.display());
        }
    }
}

/// The choices the windows share, saved to `state.json` as they change.
pub struct Prefs {
    saved: Mutex<Saved>,
    file: StateFile,
}

impl Prefs {
    pub fn load(file: StateFile) -> Self {
        Self {
            saved: Mutex::new(file.load()),
            file,
        }
    }

    fn saved(&self) -> MutexGuard<'_, Saved> {
        self.saved.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the saved state and writes it to disk.
    fn update(&self, f: impl FnOnce(&mut Saved)) {
        let mut saved = self.saved();
        f(&mut saved);
        self.file.save(&saved);
    }

    #[cfg(desktop)]
    pub fn use_mac(&self) -> bool {
        self.saved().uses_mac()
    }

    pub fn set_use_mac(&self, on: bool) {
        self.update(|s| s.set_use_mac(on));
    }

    /// Whether coding agents notify when they ask or finish a turn.
    pub fn agent_notices(&self) -> bool {
        self.saved().agent_notices
    }

    pub fn set_agent_notices(&self, on: bool) {
        self.update(|s| s.agent_notices = on);
    }

    /// Whether machines running an older beep-node update themselves.
    pub fn node_auto_update(&self) -> bool {
        self.saved().node_auto_update
    }

    pub fn set_node_auto_update(&self, on: bool) {
        self.update(|s| s.node_auto_update = on);
    }

    /// The folder picked for saves, if any.
    pub fn save_folder(&self) -> Option<PathBuf> {
        self.saved().save_folder.clone()
    }

    pub fn set_save_folder(&self, folder: Option<PathBuf>) {
        self.update(|s| s.save_folder = folder);
    }

    /// Whether app and node updates include patch releases.
    pub fn developer(&self) -> bool {
        self.saved().developer
    }

    pub fn node_release_channel(&self) -> ReleaseChannel {
        if self.developer() {
            ReleaseChannel::All
        } else {
            ReleaseChannel::Minor
        }
    }

    pub fn set_developer(&self, on: bool) {
        self.update(|s| s.developer = on);
    }

    /// The page zoom of every window.
    pub fn zoom(&self) -> Zoom {
        self.saved().zoom
    }

    pub fn set_zoom(&self, zoom: Zoom) {
        self.update(|s| s.zoom = zoom);
    }

    /// The size of the last project window.
    #[cfg(desktop)]
    pub fn project_window(&self) -> Option<LogicalSize<f64>> {
        self.saved().project_window
    }

    /// Notes a project window's size as it resizes, without writing it:
    /// `save` does when the window closes or Beep quits.
    #[cfg(desktop)]
    pub fn note_project_window(&self, size: LogicalSize<f64>) {
        self.saved().project_window = Some(size);
    }

    /// Writes what was noted.
    pub fn save(&self) {
        self.file.save(&self.saved());
    }
}

/// Writes `bytes` to `path` through a temporary file, so a reader never
/// sees half of it.
pub fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    fs::write(&partial, bytes)?;
    fs::rename(&partial, path)?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/beep_state.rs"]
mod tests;
