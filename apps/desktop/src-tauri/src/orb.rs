//! The orb: live mode while the bar is hidden. A small window in the top
//! right corner of the menu bar's screen that stays on top and never takes
//! focus. It follows the same `live` events as the bar; clicking it brings
//! the bar back, and its × cancels the reply or leaves. While the reply
//! waits for an OK, a card under the orb asks, and the window grows to fit
//! it.

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const LABEL: &str = "orb";
/// The window's side: the 64 px orb, the × on its corner, and room for its
/// glow and shadow so the window's edge never cuts them off.
const SIZE: f64 = 112.0;
/// Gap to the screen's right edge and to the menu bar. The window's own
/// padding already keeps the orb clear of both.
const MARGIN: f64 = 0.0;

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
    let width = orb
        .inner_size()?
        .to_logical::<f64>(orb.scale_factor()?)
        .width;
    place(app, &orb, width)?;
    orb.show()
}

/// Sizes the orb's window to `width` by `height`, never smaller than the
/// orb alone, keeping it in the top right corner.
pub fn fit<R: Runtime>(app: &AppHandle<R>, width: f64, height: f64) -> tauri::Result<()> {
    let Some(orb) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let size = LogicalSize::new(width.max(SIZE), height.max(SIZE));
    let now = orb
        .inner_size()?
        .to_logical::<f64>(orb.scale_factor()?)
        .width;
    // Resizing keeps the left edge, so the window moves the other way
    // first: what is past the screen's edge for a moment is never seen.
    if size.width >= now {
        orb.set_size(size)?;
        place(app, &orb, size.width)
    } else {
        place(app, &orb, size.width)?;
        orb.set_size(size)
    }
}

/// Hides the orb, if it is up.
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(orb) = app.get_webview_window(LABEL)
        && let Err(e) = orb.hide()
    {
        log::warn!("hiding the orb: {e}");
    }
}

/// Puts the orb, `width` wide, in the top right corner of the screen with
/// the menu bar.
fn place<R: Runtime>(app: &AppHandle<R>, orb: &WebviewWindow<R>, width: f64) -> tauri::Result<()> {
    let Some(monitor) = app.primary_monitor()? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let x = f64::from(area.position.x) + f64::from(area.size.width) - (width + MARGIN) * scale;
    let y = f64::from(area.position.y) + MARGIN * scale;
    orb.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}
