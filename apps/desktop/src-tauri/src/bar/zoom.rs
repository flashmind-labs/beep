//! Page zoom (⌘= ⌘- ⌘0): one level for the app, saved with its settings and
//! applied to every window as it loads and whenever it changes. The pages
//! work out the next level and hear "zoom" to fit their terminals again.

use crate::beep::{Beep, Service, state::Zoom};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime, Webview};

/// The saved level, in percent.
#[tauri::command]
pub fn zoom_level(beep: Service<'_>) -> u16 {
    beep.zoom().percent()
}

/// Saves `percent` (brought into range) and zooms every window to it.
#[tauri::command]
pub fn set_zoom_level(app: AppHandle, beep: Service<'_>, percent: u16) -> u16 {
    let zoom = Zoom::new(percent);
    beep.set_zoom(zoom);
    apply_all(&app, zoom);
    if let Err(e) = app.emit("zoom", zoom.percent()) {
        log::warn!("telling the windows about the zoom: {e}");
    }
    zoom.percent()
}

/// Zooms a webview that just loaded a page to the saved level. Pages that
/// load before the app is set up are zoomed by `apply_saved`.
pub(crate) fn on_load<R: Runtime>(webview: &Webview<R>) {
    let Some(beep) = webview.try_state::<Arc<Beep>>() else {
        return;
    };
    if let Err(e) = webview.set_zoom(beep.zoom().factor()) {
        log::warn!("zooming {}: {e}", webview.label());
    }
}

/// Zooms every open window to the saved level, once the app is set up.
pub(crate) fn apply_saved(app: &AppHandle, beep: &Beep) {
    apply_all(app, beep.zoom());
}

/// A window that fails keeps its zoom; the others still change.
fn apply_all(app: &AppHandle, zoom: Zoom) {
    for (label, window) in app.webview_windows() {
        if let Err(e) = window.set_zoom(zoom.factor()) {
            log::warn!("zooming {label}: {e}");
        }
    }
}
