//! The orb: live mode while the bar is hidden. A small window in the top
//! right corner of the menu bar's screen that stays on top and never takes
//! focus. It follows the same `live` events as the bar; clicking it brings
//! the bar back, and its × cancels the reply or leaves.

use tauri::{
    AppHandle, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub const LABEL: &str = "orb";
/// The window's side, room for the orb and the × on its corner.
const SIZE: f64 = 76.0;
/// Gap to the screen's right edge and to the menu bar.
const MARGIN: f64 = 8.0;

/// Shows the orb, making its window the first time.
pub fn show<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let orb = match app.get_webview_window(LABEL) {
        Some(orb) => orb,
        None => WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("orb.html".into()))
            .title("Beep")
            .inner_size(SIZE, SIZE)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible_on_all_workspaces(true)
            .accept_first_mouse(true)
            .focused(false)
            .visible(false)
            .build()?,
    };
    orb.set_focusable(false)?;
    place(app, &orb)?;
    orb.show()
}

/// Hides the orb, if it is up.
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(orb) = app.get_webview_window(LABEL)
        && let Err(e) = orb.hide()
    {
        log::warn!("hiding the orb: {e}");
    }
}

/// Puts the orb in the top right corner of the screen with the menu bar.
fn place<R: Runtime>(app: &AppHandle<R>, orb: &WebviewWindow<R>) -> tauri::Result<()> {
    let Some(monitor) = app.primary_monitor()? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let x = f64::from(area.position.x) + f64::from(area.size.width) - (SIZE + MARGIN) * scale;
    let y = f64::from(area.position.y) + MARGIN * scale;
    orb.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}
