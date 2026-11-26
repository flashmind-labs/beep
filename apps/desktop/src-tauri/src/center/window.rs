//! The command center windows: the main one, hidden instead of closed, and
//! one per project or assistant chat, labelled by what it shows. Entry
//! points: `open`, `center_open_window`, `hide_on_close`, `is_center`.

use crate::store::window_label::{Alone, alone};
use crate::{bar, beep::state::Prefs, error::Result};
use beep_proto::watched;
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};
#[cfg(target_os = "macos")]
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalSize, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
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
/// How far each project or chat window opens down and right of the one before.
const CASCADE: f64 = 28.0;
/// Told to a command center page when its window hides, minimizes, is
/// covered or goes to the background, and when it shows again, which pauses
/// and resumes what it draws.
const HIDDEN_EVENT: &str = "center-hidden";
const SHOWN_EVENT: &str = "center-shown";
const REOPENED_EVENT: &str = "center-reopened";

/// Where a window opens on the screen under the mouse: at `size`, or
/// `SHARE` of the screen without one, at least the minimum where the
/// screen allows, centered.
#[cfg_attr(mobile, allow(dead_code))]
fn frame<R: Runtime>(
    app: &AppHandle<R>,
    size: Option<LogicalSize<f64>>,
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
    let size = size.unwrap_or(LogicalSize::new(full_w * SHARE, full_h * SHARE));
    let width = size.width.max(MIN_WIDTH).min(full_w);
    let height = size.height.max(MIN_HEIGHT).min(full_h);
    let x = left + ((full_w - width) / 2.0).min(full_w - width).max(0.0);
    let y = top + ((full_h - height) / 2.0).min(full_h - height).max(0.0);
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
fn build<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    title: &str,
    offset: f64,
    focus: bool,
) -> Result<()> {
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
            .focused(focus)
            .maximizable(true)
            .min_inner_size(MIN_WIDTH, MIN_HEIGHT);
    }
    #[cfg(desktop)]
    if !focus {
        builder = builder.initialization_script("window.__BEEP_RESTORING_WINDOW__ = true;");
    }
    // The UI reads this to know it runs on a phone, before its scripts load.
    #[cfg(mobile)]
    {
        builder = builder.initialization_script("window.__BEEP_PHONE__ = true;");
    }
    // The last Home is on the page before its scripts run.
    #[cfg(desktop)]
    if label == LABEL
        && let Some(frame) = super::home_frame::load(app)
    {
        builder = builder.initialization_script(super::home_frame::script(&frame));
    }
    #[cfg(target_os = "macos")]
    {
        // The window is the system's sidebar material; the page tells its styles so
        // before they load, and draws the sidebar straight on it (dist/native.css).
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            .transparent(true)
            .effects(
                EffectsBuilder::new()
                    .effect(Effect::Sidebar)
                    .state(EffectState::FollowsWindowActiveState)
                    .build(),
            )
            .initialization_script("document.documentElement.dataset.material = 'vibrant';");
    }
    // Windows and Linux draw their own title bar, light when the system is: it
    // starts dark like the default skin, and the page sets it for a light one.
    #[cfg(all(desktop, not(target_os = "macos")))]
    {
        builder = builder.theme(Some(tauri::Theme::Dark));
    }
    // A project window takes the last one's size; one opened before
    // comes back where it was (the window-state plugin).
    #[cfg(desktop)]
    {
        let size = app
            .try_state::<Prefs>()
            .filter(|_| alone(label))
            .and_then(|prefs| prefs.project_window());
        builder = match frame(app, size) {
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
    #[cfg(desktop)]
    if alone(label) {
        let window = window.as_ref().window();
        app.run_on_main_thread(move || crate::store::window_session::opened(&window))?;
    }
    if focus {
        window.set_focus()?;
    }
    Ok(())
}

/// Shows the window, opening it if needed. A window that was closed is
/// only hidden, so it comes back with what it showed.
pub fn open<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    bar::hide(app);
    let Some(window) = app.get_webview_window(LABEL) else {
        return build(app, LABEL, "Beep", 0.0, true);
    };

    if !window.is_visible()? {
        join_dock(app);
        bring_up(&window)?;
        // WKWebView may still report a hidden window as visible, so the
        // page hears it from here.
        tell(app, LABEL, true, Tell::Always);
        if let Err(e) = app.emit_to(LABEL, REOPENED_EVENT, ()) {
            log::warn!("telling the command center it reopened: {e}");
        }
        return Ok(());
    }

    bring_up(&window)
}

/// Closing the main window hides it instead, so it keeps its page and data
/// and reopens at once. Its data keeps syncing slowly while hidden.
/// False for any other window, which closes.
pub(crate) fn hide_on_close<R: Runtime>(window: &Window<R>) -> bool {
    if window.label() != LABEL {
        return false;
    }

    let app = window.app_handle();

    if let Err(e) = window.hide() {
        log::warn!("hiding the window: {e}");
        return false;
    }
    // Only the focused window holds notices back.
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

/// Whether a command center window, the main one or a project's or chat's, is on
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

/// Whether `label` is a command center window: the main one, a project's or a chat's.
pub(crate) fn is_center(label: &str) -> bool {
    label == LABEL || alone(label)
}

/// What opening a project or a chat in its own window does.
#[derive(Debug, PartialEq, Eq)]
enum Pick {
    /// Its window is open already: bring that one up.
    Focus(String),
    /// A new window, `cascade` project windows being open already, so it
    /// opens a step further down than the last and none hides another.
    Open { label: String, cascade: u32 },
}

/// What to do for project `name`, given the labels of the windows open.
fn pick<'a>(alone: &Alone, open: impl IntoIterator<Item = &'a str>) -> Pick {
    let label = alone.label();
    let mut cascade: u32 = 0;

    for open in open {
        if open == label {
            return Pick::Focus(label);
        }

        if self::alone(open) {
            cascade = cascade.saturating_add(1);
        }
    }

    Pick::Open { label, cascade }
}

/// Opens a project or a chat in a window of its own, or brings up the one
/// it has. The window keeps its own history, and closing it leaves the rest.
// WebView2 must build the new webview outside the calling window's IPC handler.
#[cfg_attr(windows, tauri::command(async))]
#[cfg_attr(not(windows), tauri::command)]
pub fn center_open_window(app: AppHandle, alone: Alone) -> Result<()> {
    bar::hide(&app);
    let labels: Vec<String> = app.webview_windows().into_keys().collect();

    match pick(&alone, labels.iter().map(String::as_str)) {
        Pick::Focus(label) => match app.get_webview_window(&label) {
            Some(window) => bring_up(&window),
            None => build(&app, &label, alone.title(), CASCADE, true),
        },
        Pick::Open { label, cascade } => {
            // The first step is already one in, off the main window.
            let steps = f64::from(cascade % 8 + 1);
            build(&app, &label, alone.title(), CASCADE * steps, true)
        }
    }
}

#[cfg(desktop)]
pub(crate) fn restore_detached(app: &AppHandle) {
    for target in app
        .state::<crate::store::window_session::WindowSession>()
        .open_windows()
    {
        let label = target.label();
        if app.get_webview_window(&label).is_none()
            && let Err(error) = build(app, &label, target.title(), CASCADE, false)
        {
            log::warn!("reopening a detached window: {error}");
        }
    }
}

/// What the calling window shows alone, `None` in the main window.
#[tauri::command]
pub fn center_window_alone(window: WebviewWindow) -> Option<Alone> {
    Alone::of(window.label())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn closed<R: Runtime>(_: &AppHandle<R>, _: &str) {}

/// Whether each command center page was last told its window shows, `None`
/// before it was told anything. Native work for a window waits on it too.
#[derive(Default)]
pub(crate) struct OnScreen(Mutex<HashMap<String, watch::Sender<Option<bool>>>>);

impl OnScreen {
    fn told(&self) -> MutexGuard<'_, HashMap<String, watch::Sender<Option<bool>>>> {
        crate::store::lock(&self.0)
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
    watched::read(shown, |shown| *shown == Some(false))
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

    // Back from the background, or from Control Center, which sends only
    // focus: the links come back as well.
    #[cfg(mobile)]
    if let Some(fleet) = window
        .app_handle()
        .try_state::<std::sync::Arc<crate::fleet::Fleet>>()
    {
        match event {
            WindowEvent::Suspended => fleet.went_away(),
            WindowEvent::Resumed | WindowEvent::Focused(true) => {
                let fleet = fleet.inner().clone();
                tauri::async_runtime::spawn(async move { fleet.came_back().await });
            }
            _ => {}
        }
    }

    #[cfg(desktop)]
    if let WindowEvent::Resized(size) = event {
        note_project_size(window, *size);
    }

    #[cfg(desktop)]
    if matches!(
        event,
        WindowEvent::Focused(_) | WindowEvent::Moved(_) | WindowEvent::Resized(_)
    ) {
        crate::store::window_session::changed(window);
    }

    if let Some((shown, when)) = shown_after(event, || on_screen(window)) {
        tell(window.app_handle(), window.label(), shown, when);
    }
}

/// Notes a project window's size for the next project window, unless it
/// fills the screen or is minimized.
#[cfg(desktop)]
fn note_project_size<R: Runtime>(window: &Window<R>, size: PhysicalSize<u32>) {
    let filled = || {
        window.is_maximized().unwrap_or(true)
            || window.is_fullscreen().unwrap_or(true)
            || window.is_minimized().unwrap_or(true)
    };

    if !alone(window.label()) || size.width == 0 || filled() {
        return;
    }

    if let (Some(prefs), Ok(scale)) = (
        window.app_handle().try_state::<Prefs>(),
        window.scale_factor(),
    ) {
        prefs.note_project_window(size.to_logical(scale));
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
        crate::store::window_session::changed(&watched);
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

    // The next project window opens at the size this one had.
    if alone(label)
        && let Some(prefs) = app.try_state::<Prefs>()
    {
        prefs.save();
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
