//! The bar's window: showing it on the screen under the mouse, sizing it to
//! the page, and handing focus back when it hides.

use crate::{error::Result, voice::orb};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow};

pub(crate) const LABEL: &str = "bar";
const WIDTH: f64 = 680.0;
const MIN_HEIGHT: f64 = 64.0;
/// Share of the screen's height the bar may grow to.
const MAX_SHARE: f64 = 0.7;
/// Where the bar's top sits, as a share of the screen's height.
const TOP: f64 = 0.22;

fn window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(LABEL)
}

/// Shows the bar on the screen under the mouse, or hides it if it is open.
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let Some(bar) = window(app) else { return };
    if bar.is_visible().unwrap_or(false) {
        dismiss(app);
    } else {
        show(app, None);
    }
}

/// Shows the bar, in place of the orb when live mode runs. `view` asks the
/// page to open a view, such as "link".
pub fn show<R: Runtime>(app: &AppHandle<R>, view: Option<&str>) {
    let Some(bar) = window(app) else { return };
    if let Err(e) = place(app, &bar) {
        log::warn!("placing the bar: {e}");
    }
    orb::hide(app);
    let _ = bar.show();
    activate();
    let _ = bar.set_focus();
    let _ = app.emit_to(LABEL, "bar-shown", view);
}

/// Makes Beep the active app, so the bar it shows gets the keyboard. A
/// tray app opened from a hotkey is otherwise left behind the app in front:
/// the bar shows but typing and Escape still go there.
#[cfg(target_os = "macos")]
fn activate() {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    // macOS 14 and later ignore IgnoringOtherApps and activate on the
    // user's keypress anyway; 13 needs it.
    #[allow(deprecated)]
    let options = NSApplicationActivationOptions::ActivateAllWindows
        | NSApplicationActivationOptions::ActivateIgnoringOtherApps;
    if !NSRunningApplication::currentApplication().activateWithOptions(options) {
        log::warn!("macOS did not make Beep the active app");
    }
}

#[cfg(not(target_os = "macos"))]
fn activate() {}

/// Whether the bar has the keyboard.
pub fn is_focused<R: Runtime>(app: &AppHandle<R>) -> bool {
    window(app)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// Whether the bar is on screen.
pub fn is_open<R: Runtime>(app: &AppHandle<R>) -> bool {
    window(app)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Hides the bar and tells its page, which stops what it follows on
/// screen (WKWebView may still report a hidden window as visible).
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(bar) = window(app) {
        let _ = bar.hide();
        let _ = app.emit_to(LABEL, "bar-hidden", ());
    }
}

/// Hides the bar the user is done with. It took focus when it opened, so
/// focus goes back to the app the user was in, unless another window is
/// still up.
fn dismiss<R: Runtime>(app: &AppHandle<R>) {
    let focused = window(app).is_some_and(|bar| bar.is_focused().unwrap_or(false));
    hide(app);
    let others = app
        .webview_windows()
        .into_values()
        .any(|w| w.label() != LABEL && w.is_visible().unwrap_or(false));
    if focused && !others {
        give_back_focus(app);
    }
}

#[cfg(target_os = "macos")]
fn give_back_focus<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = app.hide() {
        log::warn!("handing focus back: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
fn give_back_focus<R: Runtime>(_: &AppHandle<R>) {}

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
fn resize<R: Runtime>(app: &AppHandle<R>, height: f64) -> tauri::Result<()> {
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

#[tauri::command]
pub fn resize_bar(app: AppHandle, height: f64) -> Result<()> {
    Ok(resize(&app, height)?)
}

#[tauri::command]
pub fn hide_bar(app: AppHandle) {
    dismiss(&app);
}

/// A click on the orb: the bar comes back in its place.
#[tauri::command]
pub fn show_bar(app: AppHandle) {
    show(&app, None);
}
