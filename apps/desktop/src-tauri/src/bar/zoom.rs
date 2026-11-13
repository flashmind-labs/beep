//! Page zoom (⌘= ⌘- ⌘0): one level for the app, saved with its settings and
//! applied to every window as it loads and whenever it changes. The pages
//! work out the next level and hear "zoom" to fit their terminals again.

use crate::beep::state::{Prefs, Zoom};
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Webview};

/// The saved level, in percent.
#[tauri::command]
pub fn zoom_level(prefs: State<'_, Prefs>) -> u16 {
    prefs.zoom().percent()
}

/// Saves `percent` (brought into range) and zooms every window to it.
#[tauri::command]
pub fn set_zoom_level(app: AppHandle, prefs: State<'_, Prefs>, percent: u16) -> u16 {
    let zoom = Zoom::new(percent);
    prefs.set_zoom(zoom);
    apply_all(&app, zoom);

    if let Err(e) = app.emit("zoom", zoom.percent()) {
        log::warn!("telling the windows about the zoom: {e}");
    }

    zoom.percent()
}

/// Zooms a webview that just loaded a page to the saved level.
pub(crate) fn on_load<R: Runtime>(webview: &Webview<R>) {
    // A phone has no zoom setting, and WKWebView's zoom shrinks the page there.
    if cfg!(mobile) {
        return;
    }

    let Some(prefs) = webview.try_state::<Prefs>() else {
        return;
    };

    if let Err(e) = webview.set_zoom(prefs.zoom().factor()) {
        log::warn!("zooming {}: {e}", webview.label());
    }
}

/// A window that fails keeps its zoom; the others still change.
fn apply_all(app: &AppHandle, zoom: Zoom) {
    if cfg!(mobile) {
        return;
    }

    for (label, window) in app.webview_windows() {
        if let Err(e) = window.set_zoom(zoom.factor()) {
            log::warn!("zooming {label}: {e}");
        }
    }
}
