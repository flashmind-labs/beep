//! The bar: one borderless window that shows and hides in place.

use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow};

pub const WIDTH: f64 = 680.0;
const MIN_HEIGHT: f64 = 64.0;
/// Share of the screen's height the bar may grow to.
const MAX_SHARE: f64 = 0.7;
/// Where the bar's top sits, as a share of the screen's height.
const TOP: f64 = 0.22;

fn window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window("bar")
}

/// Shows the bar on the screen under the mouse, or hides it if it is open.
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let Some(bar) = window(app) else { return };
    if bar.is_visible().unwrap_or(false) {
        hide(app);
    } else {
        show(app, None);
    }
}

/// Shows the bar. `view` asks the page to open a view, such as "link".
pub fn show<R: Runtime>(app: &AppHandle<R>, view: Option<&str>) {
    let Some(bar) = window(app) else { return };
    if let Err(e) = place(app, &bar) {
        log::warn!("placing the bar: {e}");
    }
    let _ = bar.show();
    let _ = bar.set_focus();
    let _ = app.emit_to("bar", "bar-shown", view);
}

pub fn hide<R: Runtime>(app: &AppHandle<R>) {
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
