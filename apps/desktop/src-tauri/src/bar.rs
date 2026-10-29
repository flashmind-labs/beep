//! The bar: one borderless window that shows and hides in place.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow};

pub const LABEL: &str = "bar";
pub const WIDTH: f64 = 680.0;
const MIN_HEIGHT: f64 = 64.0;
/// Share of the screen's height the bar may grow to.
const MAX_SHARE: f64 = 0.7;
/// Where the bar's top sits, as a share of the screen's height.
const TOP: f64 = 0.22;

/// The bar is up for a voice note without taking focus, so whatever the
/// user is typing in keeps the keyboard.
static PASSIVE: AtomicBool = AtomicBool::new(false);

fn window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(LABEL)
}

/// Shows the bar on the screen under the mouse, or hides it if it is open.
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let Some(bar) = window(app) else { return };
    if bar.is_visible().unwrap_or(false) && !is_passive() {
        hide(app);
    } else {
        show(app, None);
    }
}

/// Shows the bar. `view` asks the page to open a view, such as "link".
pub fn show<R: Runtime>(app: &AppHandle<R>, view: Option<&str>) {
    let Some(bar) = window(app) else { return };
    // A bar up for a voice note stays where it is and takes focus.
    if !PASSIVE.swap(false, Ordering::Relaxed)
        && let Err(e) = place(app, &bar)
    {
        log::warn!("placing the bar: {e}");
    }
    let _ = bar.set_focusable(true);
    let _ = bar.show();
    let _ = bar.set_focus();
    let _ = app.emit_to(LABEL, "bar-shown", view);
}

/// Shows the bar for a voice note without taking focus. Does nothing when
/// the bar is already open for the user. True when the bar is up passively.
pub fn show_passive<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(bar) = window(app) else { return false };
    if is_passive() {
        return true;
    }
    if bar.is_visible().unwrap_or(false) {
        return false;
    }
    if let Err(e) = place(app, &bar) {
        log::warn!("placing the bar: {e}");
    }
    let _ = bar.set_focusable(false);
    PASSIVE.store(true, Ordering::Relaxed);
    let _ = bar.show();
    true
}

/// Whether the bar is up for a voice note, not opened by the user.
pub fn is_passive() -> bool {
    PASSIVE.load(Ordering::Relaxed)
}

/// Whether the bar is on screen.
pub fn is_open<R: Runtime>(app: &AppHandle<R>) -> bool {
    window(app)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    PASSIVE.store(false, Ordering::Relaxed);
    if let Some(bar) = window(app) {
        let _ = bar.hide();
    }
}

fn place<R: Runtime>(app: &AppHandle<R>, bar: &WebviewWindow<R>) -> tauri::Result<()> {
    let cursor = app.cursor_position()?;
    let monitor = match app.monitor_from_point(cursor.x, cursor.y)? {
        Some(monitor) => monitor,
        None => match bar.primary_monitor()? {
            Some(monitor) => monitor,
            None => return Ok(()),
        },
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let width = WIDTH * scale;
    let x = f64::from(area.position.x) + (f64::from(area.size.width) - width) / 2.0;
    let y = f64::from(area.position.y) + f64::from(area.size.height) * TOP;
    bar.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}

/// Sets the bar's height to fit the page, within the screen.
pub fn resize<R: Runtime>(app: &AppHandle<R>, height: f64) -> tauri::Result<()> {
    let Some(bar) = window(app) else {
        return Ok(());
    };
    let max = match bar.current_monitor()? {
        Some(monitor) => {
            f64::from(monitor.work_area().size.height) / monitor.scale_factor() * MAX_SHARE
        }
        None => 600.0,
    };
    bar.set_size(LogicalSize::new(WIDTH, height.clamp(MIN_HEIGHT, max)))
}
