//! The command center windows: the main one, hidden instead of closed, and
//! one per project, labelled by the project's name. Entry points: `open`,
//! `open_project`, `hide_on_close`, `is_center`.

use crate::{bar, error::Result};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};
use tokio::sync::watch;

pub const LABEL: &str = "center";
/// The window never shrinks below this.
#[cfg_attr(mobile, allow(dead_code))]
const MIN_WIDTH: f64 = 1000.0;
#[cfg_attr(mobile, allow(dead_code))]
const MIN_HEIGHT: f64 = 640.0;
/// Share of the screen the window opens at.
#[cfg_attr(mobile, allow(dead_code))]
const SHARE: f64 = 0.95;
/// Labels of the windows that each show one project.
const PROJECT_PREFIX: &str = "project-";
/// How far each project window opens down and right of the one before.
const CASCADE: f64 = 28.0;
/// Told to a command center page when its window hides, minimizes, is
/// covered or goes to the background, and when it shows again, which pauses
/// and resumes what it draws.
const HIDDEN_EVENT: &str = "center-hidden";
const SHOWN_EVENT: &str = "center-shown";

/// Where a window opens: `share` of the screen under the mouse, at least
/// `min` where the screen allows, centered, then moved down and right by
/// `step` as far as the screen allows.
#[cfg_attr(mobile, allow(dead_code))]
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
    #[cfg(desktop)]
    if window.is_minimized()? {
        window.unminimize()?;
    }

    window.show()?;
    window.set_focus()?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn join_dock<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
        log::warn!("joining the Dock: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn join_dock<R: Runtime>(_: &AppHandle<R>) {}

/// Builds command center window `label` titled `title`, moved down and
/// right by `offset` from where the main window opens. While it is open
/// Beep sits in the Dock.
#[cfg_attr(mobile, allow(dead_code, unused_variables, unused_mut))]
fn build<R: Runtime>(app: &AppHandle<R>, label: &str, title: &str, offset: f64) -> Result<()> {
    join_dock(app);
    let mut builder = crate::navigation::guard(WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App("center.html".into()),
    ))
    .title(title)
    .resizable(true)
    // Files dragged in from Finder reach the page as HTML drops, as in
    // the bar: a coding agent session uploads them to its machine.
    .disable_drag_drop_handler();
    // A phone window fills the screen: a desktop minimum leaves its webview at zero size.
    #[cfg(desktop)]
    {
        builder = builder
            .maximizable(true)
            .min_inner_size(MIN_WIDTH, MIN_HEIGHT);
    }
    // The UI reads this to know it runs on a phone, before its scripts load.
    #[cfg(mobile)]
    {
        builder = builder.initialization_script("window.__BEEP_PHONE__ = true;");
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }
    #[cfg(desktop)]
    {
        builder = match frame(app, SHARE, LogicalSize::new(MIN_WIDTH, MIN_HEIGHT), 0.0) {
            Some((position, size)) => builder
                .position(position.x + offset, position.y + offset)
                .inner_size(size.width, size.height),
            None => builder.inner_size(MIN_WIDTH, MIN_HEIGHT).center(),
        };
    }
    let window = builder.build()?;

    if let Err(e) = watch_occlusion(&window) {
        log::warn!("watching whether {label} is covered: {e}");
    }

    #[cfg(target_os = "ios")]
    window.with_webview(|webview| crate::platform::ios_keyboard::hide_bar(webview.inner()))?;
    window.set_focus()?;
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
        tell(app, LABEL, true, Tell::Always);
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
    tell(app, LABEL, false, Tell::Always);
    closed(app, LABEL);
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

/// Whether a command center window, the main one or a project's, is on
/// screen, leaving out `closing`, which may still be listed. The main
/// window hides when closed.
#[cfg(target_os = "macos")]
fn center_open<R: Runtime>(app: &AppHandle<R>, closing: &str) -> bool {
    app.webview_windows().iter().any(|(label, window)| {
        label != closing && is_center(label) && window.is_visible().unwrap_or(true)
    })
}

/// Command center windows are ordinary windows: while one is open Beep sits
/// in the Dock and Cmd-Tab, and goes back to the menu bar once the last
/// closes.
#[cfg(target_os = "macos")]
pub(crate) fn closed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if is_center(label)
        && !center_open(app, label)
        && let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Accessory)
    {
        log::warn!("leaving the Dock: {e}");
    }
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

#[cfg(not(target_os = "macos"))]
pub(crate) fn closed<R: Runtime>(_: &AppHandle<R>, _: &str) {}

/// Whether each command center page was last told its window shows, `None`
/// before it was told anything. Native work for a window waits on it too.
#[derive(Default)]
pub(crate) struct OnScreen(Mutex<HashMap<String, watch::Sender<Option<bool>>>>);

impl OnScreen {
    fn told(&self) -> MutexGuard<'_, HashMap<String, watch::Sender<Option<bool>>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records that `label` shows or not, true if that changed.
    fn record(&self, label: &str, shown: bool) -> bool {
        let mut told = self.told();
        let sender = told
            .entry(label.to_owned())
            .or_insert_with(|| watch::Sender::new(None));
        sender.send_replace(Some(shown)) != Some(shown)
    }

    /// Whether window `label` shows, as it changes. The receiver closes with
    /// the window.
    pub(crate) fn watch(&self, label: &str) -> watch::Receiver<Option<bool>> {
        self.told()
            .entry(label.to_owned())
            .or_insert_with(|| watch::Sender::new(None))
            .subscribe()
    }

    /// The window closed for good.
    pub(crate) fn clear(&self, label: &str) {
        self.told().remove(label);
    }
}

/// Whether a window `OnScreen::watch` follows is hidden. One never told is
/// taken as shown.
pub(crate) fn hidden(shown: &watch::Receiver<Option<bool>>) -> bool {
    *shown.borrow() == Some(false)
}

/// Whether a page hears its window's state again when nothing changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tell {
    /// A single event the page may have missed, or a page that was
    /// reloaded since it last heard.
    Always,
    /// One of a burst, such as a resize while dragging.
    #[cfg_attr(mobile, allow(dead_code))]
    Changed,
}

/// Tells page `label` whether its window is on screen.
fn tell<R: Runtime>(app: &AppHandle<R>, label: &str, shown: bool, when: Tell) {
    let changed = app
        .try_state::<OnScreen>()
        .is_none_or(|told| told.record(label, shown));

    if !changed && when == Tell::Changed {
        return;
    }

    #[cfg(windows)]
    webview_shows(app, label, shown);

    let event = if shown { SHOWN_EVENT } else { HIDDEN_EVENT };

    if let Err(e) = app.emit_to(label, event, ()) {
        log::warn!("telling {label} whether it shows: {e}");
    }
}

/// Tells WebView2 whether window `label` shows. It throttles the page's
/// timers and frees memory only while told it is hidden; a minimized or
/// hidden window does not tell it.
#[cfg(windows)]
fn webview_shows<R: Runtime>(app: &AppHandle<R>, label: &str, shown: bool) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let told = window.with_webview(move |webview| {
        // SAFETY: Tauri hands out the webview's live controller and runs
        // this on the main thread, where WebView2 expects its calls.
        if let Err(e) = unsafe { webview.controller().SetIsVisible(shown) } {
            log::warn!("telling WebView2 whether its window shows: {e}");
        }
    });

    if let Err(e) = told {
        log::warn!("reaching the webview of {label}: {e}");
    }
}

/// What a window event says about whether a command center window shows:
/// `None` when it says nothing. `on_screen` asks the window system.
fn shown_after(event: &WindowEvent, on_screen: impl FnOnce() -> bool) -> Option<(bool, Tell)> {
    // Phones learn it from suspend and resume alone.
    #[cfg(mobile)]
    let _ = on_screen;

    match event {
        // A phone app going to the background, and coming back. iOS sends
        // only focus when the app returns from Control Center.
        #[cfg(mobile)]
        WindowEvent::Suspended => Some((false, Tell::Always)),
        #[cfg(mobile)]
        WindowEvent::Resumed | WindowEvent::Focused(true) => Some((true, Tell::Always)),
        #[cfg(mobile)]
        _ => None,
        // Minimizing and restoring resize the window on Linux and Windows;
        // macOS says so through occlusion (`watch_occlusion`).
        #[cfg(desktop)]
        WindowEvent::Focused(true) => Some((true, Tell::Always)),
        #[cfg(desktop)]
        WindowEvent::Focused(false) | WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
            Some((on_screen(), Tell::Changed))
        }
        #[cfg(desktop)]
        _ => None,
    }
}

/// Follows a command center window's events, so its page pauses while it
/// is minimized, covered or in the background.
pub(crate) fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if !is_center(window.label()) {
        return;
    }

    if let Some((shown, when)) = shown_after(event, || on_screen(window)) {
        tell(window.app_handle(), window.label(), shown, when);
    }
}

/// Whether `window` is on screen: shown and not minimized.
#[cfg(all(desktop, not(target_os = "macos")))]
fn on_screen<R: Runtime>(window: &Window<R>) -> bool {
    window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false)
}

/// Whether any part of `window` is on screen: not hidden, minimized, fully
/// covered or on another Space. Main thread only.
#[cfg(target_os = "macos")]
fn on_screen<R: Runtime>(window: &Window<R>) -> bool {
    use objc2_app_kit::{NSWindow, NSWindowOcclusionState};

    let Ok(ns_window) = window.ns_window() else {
        return true;
    };
    // SAFETY: Tauri hands out the window's live NSWindow, and this runs on
    // the main thread, from a window event or an AppKit notification.
    let ns_window = unsafe { &*ns_window.cast::<NSWindow>() };
    ns_window
        .occlusionState()
        .contains(NSWindowOcclusionState::Visible)
}

#[cfg(mobile)]
fn on_screen<R: Runtime>(_: &Window<R>) -> bool {
    true
}

/// What `NSNotificationCenter` returns to remove an observer with.
#[cfg(target_os = "macos")]
type Observer =
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>;

#[cfg(target_os = "macos")]
thread_local! {
    /// The occlusion observer of each command center window. Main thread only.
    static OCCLUSION: std::cell::RefCell<HashMap<String, Observer>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Tells the page when its window is minimized, covered, on another Space
/// or back, which macOS reports only as occlusion. Main thread only.
#[cfg(target_os = "macos")]
fn watch_occlusion<R: Runtime>(window: &WebviewWindow<R>) -> Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSWindow, NSWindowDidChangeOcclusionStateNotification};
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use std::ptr::NonNull;

    let ns_window = window.ns_window()?;
    let watched = window.as_ref().window();
    let label = watched.label().to_owned();
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        tell(
            watched.app_handle(),
            watched.label(),
            on_screen(&watched),
            Tell::Changed,
        );
    });
    // SAFETY: the object is the window's live NSWindow. With no queue the
    // block runs where AppKit posts the notification, on the main thread,
    // and `forget` removes it before the window goes.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSWindowDidChangeOcclusionStateNotification),
            Some(&*ns_window.cast::<NSWindow>()),
            None,
            &block,
        )
    };
    OCCLUSION.with_borrow_mut(|observers| observers.insert(label, observer));
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn watch_occlusion<R: Runtime>(_: &WebviewWindow<R>) -> Result<()> {
    Ok(())
}

/// Window `label` is gone: forgets what its page was told and stops
/// watching it.
pub(crate) fn forget<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(told) = app.try_state::<OnScreen>() {
        told.clear(label);
    }

    #[cfg(target_os = "macos")]
    if let Some(observer) = OCCLUSION.with_borrow_mut(|observers| observers.remove(label)) {
        let observer: &objc2::runtime::AnyObject = (*observer).as_ref();
        // SAFETY: the observer came from `addObserverForName` on this center.
        unsafe { objc2_foundation::NSNotificationCenter::defaultCenter().removeObserver(observer) };
    }
}

#[cfg(test)]
#[path = "../tests/center_window.rs"]
mod tests;
