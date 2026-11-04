//! The command center windows: the main one, hidden instead of closed, and
//! one per project, labelled by the project's name. Entry points: `open`,
//! `open_project`, `hide_on_close`, `is_center`.

use crate::{bar, error::Result, research};
use tauri::{
    ActivationPolicy, AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime,
    TitleBarStyle, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window,
};

pub const LABEL: &str = "center";
/// The window never shrinks below this.
const MIN_WIDTH: f64 = 1000.0;
const MIN_HEIGHT: f64 = 640.0;
/// Share of the screen the window opens at.
const SHARE: f64 = 0.95;
/// Labels of the windows that each show one project.
const PROJECT_PREFIX: &str = "project-";
/// How far each project window opens down and right of the one before.
const CASCADE: f64 = 28.0;
/// Told to the main window's page when it hides and shows again, which
/// sets how often it reads in the background.
const HIDDEN_EVENT: &str = "center-hidden";
const SHOWN_EVENT: &str = "center-shown";

/// Where a window opens: `share` of the screen under the mouse, at least
/// `min` where the screen allows, centered, then moved down and right by
/// `step` as far as the screen allows.
pub(crate) fn frame<R: Runtime>(
    app: &AppHandle<R>,
    share: f64,
    min: LogicalSize<f64>,
    step: f64,
) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let cursor = app.cursor_position().ok()?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let left = f64::from(area.position.x) / scale;
    let top = f64::from(area.position.y) / scale;
    let full_w = f64::from(area.size.width) / scale;
    let full_h = f64::from(area.size.height) / scale;
    let width = (full_w * share).max(min.width.min(full_w));
    let height = (full_h * share).max(min.height.min(full_h));
    let x = left + ((full_w - width) / 2.0 + step).min(full_w - width).max(0.0);
    let y = top
        + ((full_h - height) / 2.0 + step)
            .min(full_h - height)
            .max(0.0);
    Some((LogicalPosition::new(x, y), LogicalSize::new(width, height)))
}

fn bring_up<R: Runtime>(window: &WebviewWindow<R>) -> Result<()> {
    if window.is_minimized()? {
        window.unminimize()?;
    }
    window.show()?;
    window.set_focus()?;
    Ok(())
}

pub(crate) fn join_dock<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = app.set_activation_policy(ActivationPolicy::Regular) {
        log::warn!("joining the Dock: {e}");
    }
}

/// Builds command center window `label` titled `title`, moved down and
/// right by `offset` from where the main window opens. While it is open
/// Beep sits in the Dock, as with research windows.
fn build<R: Runtime>(app: &AppHandle<R>, label: &str, title: &str, offset: f64) -> Result<()> {
    join_dock(app);
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("center.html".into()))
        .title(title)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .maximizable(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        // Files dragged in from Finder reach the page as HTML drops, as in
        // the bar: a coding agent session uploads them to its machine.
        .disable_drag_drop_handler();
    builder = match frame(app, SHARE, LogicalSize::new(MIN_WIDTH, MIN_HEIGHT), 0.0) {
        Some((position, size)) => builder
            .position(position.x + offset, position.y + offset)
            .inner_size(size.width, size.height),
        None => builder.inner_size(MIN_WIDTH, MIN_HEIGHT).center(),
    };
    builder.build()?.set_focus()?;
    Ok(())
}

/// Shows the window, opening it if needed. A window that was closed is
/// only hidden, so it comes back with what it showed.
pub fn open<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    bar::hide(app);
    let Some(window) = app.get_webview_window(LABEL) else {
        return build(app, LABEL, "Beep", 0.0);
    };
    if !window.is_visible()? {
        join_dock(app);
        bring_up(&window)?;
        // WKWebView may still report a hidden window as visible, so the
        // page hears it from here.
        if let Err(e) = app.emit_to(LABEL, SHOWN_EVENT, ()) {
            log::warn!("telling the window it shows: {e}");
        }
        return Ok(());
    }
    bring_up(&window)
}

/// Closing the main window hides it instead, so it keeps its page, data
/// and scroll and reopens at once; its data keeps syncing slowly while
/// hidden. False for any other window, which closes as before.
pub(crate) fn hide_on_close<R: Runtime>(window: &Window<R>) -> bool {
    if window.label() != LABEL {
        return false;
    }
    let app = window.app_handle();
    if let Err(e) = window.hide() {
        log::warn!("hiding the window: {e}");
        return false;
    }
    // Its chat stays in `Shown`: only the focused window holds notices back,
    // and it comes back on the same chat.
    if let Err(e) = app.emit_to(LABEL, HIDDEN_EVENT, ()) {
        log::warn!("telling the window it hid: {e}");
    }
    research::closed(app, LABEL);
    true
}

/// Cmd-W: closes the window it came from, which for the main window hides
/// it (`hide_on_close`).
#[tauri::command]
pub fn center_close_window(window: Window) -> Result<()> {
    Ok(window.close()?)
}

#[tauri::command]
pub fn open_center(app: AppHandle) -> Result<()> {
    open(&app)
}

/// Whether `label` is a command center window: the main one or a project's.
pub(crate) fn is_center(label: &str) -> bool {
    label == LABEL || label.starts_with(PROJECT_PREFIX)
}

/// The label of project `name`'s window: the prefix, then the name in hex,
/// since a label takes only letters, digits and a few signs.
pub(super) fn project_label(name: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut label = String::from(PROJECT_PREFIX);
    for byte in name.bytes() {
        label.push(char::from(DIGITS[usize::from(byte >> 4)]));
        label.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }
    label
}

/// The project a window label names, `None` for any other window.
fn label_project(label: &str) -> Option<String> {
    let hex = label.strip_prefix(PROJECT_PREFIX)?;
    if hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

/// What opening a project in its own window does.
#[derive(Debug, PartialEq, Eq)]
enum Pick {
    /// Its window is open already: bring that one up.
    Focus(String),
    /// A new window, `cascade` project windows being open already, so it
    /// opens a step further down than the last and none hides another.
    Open { label: String, cascade: u32 },
}

/// What to do for project `name`, given the labels of the windows open.
fn pick<'a>(name: &str, open: impl IntoIterator<Item = &'a str>) -> Pick {
    let label = project_label(name);
    let mut cascade: u32 = 0;
    for open in open {
        if open == label {
            return Pick::Focus(label);
        }
        if open.starts_with(PROJECT_PREFIX) {
            cascade = cascade.saturating_add(1);
        }
    }
    Pick::Open { label, cascade }
}

/// Opens project `project` in a window of its own, or brings up the one it
/// has. The window keeps its own history, and closing it leaves the rest.
#[tauri::command]
pub fn center_open_project_window(app: AppHandle, project: String) -> Result<()> {
    bar::hide(&app);
    let labels: Vec<String> = app.webview_windows().into_keys().collect();
    match pick(&project, labels.iter().map(String::as_str)) {
        Pick::Focus(label) => match app.get_webview_window(&label) {
            Some(window) => bring_up(&window),
            None => build(&app, &label, &project, CASCADE),
        },
        Pick::Open { label, cascade } => {
            // The first step is already one in, off the main window.
            let steps = f64::from(cascade % 8 + 1);
            build(&app, &label, &project, CASCADE * steps)
        }
    }
}

/// The project the calling window was opened on, `None` in the main
/// window.
#[tauri::command]
pub fn center_window_project(window: WebviewWindow) -> Option<String> {
    label_project(window.label())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_label_reads_back_as_its_name() {
        for name in ["site", "org/repo", "my project", "café.rs", "a-b_c:d"] {
            let label = project_label(name);
            let hex = label
                .strip_prefix(PROJECT_PREFIX)
                .expect("it has the prefix");
            assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()), "{label}");
            assert_eq!(label_project(&label).as_deref(), Some(name));
        }
        assert_eq!(label_project(LABEL), None);
        assert_eq!(label_project("research-1"), None);
        assert_eq!(label_project("project-"), None);
        assert_eq!(label_project("project-abc"), None);
        assert_eq!(label_project("project-zz"), None);
    }

    #[test]
    fn a_project_open_in_a_window_is_focused() {
        let site = project_label("site");
        let open = [LABEL, "bar", site.as_str(), "research-1"];
        assert_eq!(pick("site", open), Pick::Focus(site));
    }

    #[test]
    fn a_project_without_a_window_gets_a_new_one() {
        assert_eq!(
            pick("site", [LABEL, "bar"]),
            Pick::Open {
                label: project_label("site"),
                cascade: 0,
            }
        );
    }

    #[test]
    fn a_new_window_steps_past_the_other_project_windows() {
        let docs = project_label("docs");
        let api = project_label("api");
        let open = [LABEL, docs.as_str(), "research-1", api.as_str()];
        assert_eq!(
            pick("site", open),
            Pick::Open {
                label: project_label("site"),
                cascade: 2,
            }
        );
    }

    #[test]
    fn similar_names_get_their_own_windows() {
        let site = project_label("site");
        assert!(matches!(pick("site2", [site.as_str()]), Pick::Open { .. }));
        assert!(matches!(pick("sit", [site.as_str()]), Pick::Open { .. }));
    }
}
