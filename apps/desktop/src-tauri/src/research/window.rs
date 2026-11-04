//! Research windows: opening one per research, placed so none hides
//! another, and leaving the Dock once the last window closes.

use crate::{center, error::Result};
use tauri::{
    ActivationPolicy, AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, TitleBarStyle,
    WebviewUrl, WebviewWindowBuilder,
};
use uuid::Uuid;

/// Smallest size a new window opens at, before it is fitted to the screen.
const WIDTH: f64 = 1080.0;
const HEIGHT: f64 = 700.0;
/// Share of the screen a new window takes.
const SHARE: f64 = 0.82;
/// Each window opened while others are open sits this much lower and right.
const CASCADE: f64 = 28.0;

const PREFIX: &str = "research-";

fn label(session: Uuid) -> String {
    format!("{PREFIX}{}", session.simple())
}

/// Research windows open, leaving out `closing`, which may still be listed.
fn open_windows<R: Runtime>(app: &AppHandle<R>, closing: Option<&str>) -> usize {
    app.webview_windows()
        .keys()
        .filter(|label| label.starts_with(PREFIX) && Some(label.as_str()) != closing)
        .count()
}

/// Whether a command center window, the main one or a project's, is on
/// screen, leaving out `closing`. The main window hides when closed.
fn center_open<R: Runtime>(app: &AppHandle<R>, closing: &str) -> bool {
    app.webview_windows().iter().any(|(label, window)| {
        label != closing && center::is_center(label) && window.is_visible().unwrap_or(true)
    })
}

/// Research windows and the command center windows are ordinary windows:
/// while one is open Beep sits in the Dock and Cmd-Tab, and goes back to
/// the menu bar once the last closes.
pub fn closed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if (label.starts_with(PREFIX) || center::is_center(label))
        && open_windows(app, Some(label)) == 0
        && !center_open(app, label)
        && let Err(e) = app.set_activation_policy(ActivationPolicy::Accessory)
    {
        log::warn!("leaving the Dock: {e}");
    }
}

/// Where a new window goes: most of the screen under the mouse, moved down
/// and right for each window already open so none hides another.
fn frame<R: Runtime>(
    app: &AppHandle<R>,
    open: usize,
) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let cursor = app.cursor_position().ok()?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (left, top) = (
        f64::from(area.position.x) / scale,
        f64::from(area.position.y) / scale,
    );
    let (full_w, full_h) = (
        f64::from(area.size.width) / scale,
        f64::from(area.size.height) / scale,
    );
    let width = (full_w * SHARE).max(WIDTH.min(full_w));
    let height = (full_h * SHARE).max(HEIGHT.min(full_h));
    let step = CASCADE * (open % 8) as f64;
    let x = left + ((full_w - width) / 2.0 + step).min(full_w - width).max(0.0);
    let y = top
        + ((full_h - height) / 2.0 + step)
            .min(full_h - height)
            .max(0.0);
    Some((LogicalPosition::new(x, y), LogicalSize::new(width, height)))
}

/// Shows the research window for `session`, opening it if needed.
pub(super) fn open_window<R: Runtime>(app: &AppHandle<R>, session: Uuid) -> Result<()> {
    if let Some(window) = app.get_webview_window(&label(session)) {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    if let Err(e) = app.set_activation_policy(ActivationPolicy::Regular) {
        log::warn!("joining the Dock: {e}");
    }
    let url = WebviewUrl::App(format!("research.html?session={session}").into());
    let mut builder = WebviewWindowBuilder::new(app, label(session), url)
        .title("Research")
        .min_inner_size(820.0, 520.0)
        .resizable(true)
        .maximizable(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true);
    builder = match frame(app, open_windows(app, None)) {
        Some((position, size)) => builder
            .position(position.x, position.y)
            .inner_size(size.width, size.height),
        None => builder.inner_size(WIDTH, HEIGHT).center(),
    };
    let window = builder.build()?;
    window.set_focus()?;
    Ok(())
}
