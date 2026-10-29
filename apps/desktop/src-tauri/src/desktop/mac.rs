//! Desktop control on macOS: ScreenCaptureKit for screenshots, the
//! accessibility API for the tree and for pressing elements, and Core
//! Graphics events for the mouse and keyboard. Every call here blocks, so
//! the session runs them off the async workers.

use super::{
    keys,
    tree::{self, Node},
};
use beep_proto::node::desktop::{
    KeyPress, Modifier, MouseButton, Permission, Permissions, Reply, Request, Screenshot, Target,
    Tree, Window,
};
use block2::RcBlock;
use objc2::{AnyThread, runtime::AnyObject};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImageCompressionFactor, NSWorkspace,
};
use objc2_application_services::{
    AXError, AXIsProcessTrusted, AXIsProcessTrustedWithOptions, AXUIElement, AXValue, AXValueType,
    kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGSize,
};
use objc2_core_graphics::{
    CGDisplayBounds, CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventType, CGImage,
    CGMainDisplayID, CGMouseButton, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess,
    CGScrollEventUnit, CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID,
    kCGWindowBounds, kCGWindowLayer, kCGWindowName, kCGWindowOwnerName, kCGWindowOwnerPID,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};
use std::{
    process::Command,
    ptr::{self, NonNull},
    sync::mpsc::sync_channel,
    thread::sleep,
    time::Duration,
};

/// How long a screenshot may take.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);
/// JPEG quality, from 0 to 1.
const QUALITY: f64 = 0.7;
/// The most elements read from an app, and how deep; web pages go far.
const MAX_READ: usize = 4000;
const MAX_DEPTH: usize = 40;
/// How long one accessibility call may wait on a busy app.
const AX_TIMEOUT: f32 = 1.0;
/// Text goes in pieces of at most this many UTF-16 units.
const TYPE_CHUNK: usize = 20;
/// A pause between synthetic events, so apps see them in order.
const EVENT_GAP: Duration = Duration::from_millis(15);
const MAX_WINDOWS: usize = 60;

/// An element from the session's last tree.
pub struct Element {
    ax: CFRetained<AXUIElement>,
    /// Its frame in screen points.
    center: Option<CGPoint>,
    pressable: bool,
    /// How the tree named it, for the reply.
    label: String,
}

// SAFETY: an AXUIElementRef is an immutable CF object, retained and released
// atomically, and the accessibility API may be called from any thread. The
// session only uses its elements behind a mutex, one call at a time.
unsafe impl Send for Element {}

/// What one desktop session remembers between requests.
pub struct Session {
    /// Screen points per pixel of the last screenshot.
    scale: f64,
    elements: Vec<Element>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            scale: 1.0,
            elements: Vec::new(),
        }
    }
}

impl Session {
    pub fn handle(&mut self, request: Request) -> Reply {
        match request {
            Request::Screenshot { max_side } => self.screenshot(max_side),
            Request::Tree { max_elements } => self.tree(max_elements),
            Request::Click {
                target,
                button,
                count,
            } => self.click(target, button, count),
            Request::Type { text } => trusted().unwrap_or_else(|| type_text(&text)),
            Request::Key { press } => trusted().unwrap_or_else(|| key(&press)),
            Request::Scroll { at, dx, dy } => self.scroll(at, dx, dy),
            Request::Open { target } => open(&target),
            Request::Windows {} => windows(),
            Request::Permissions {} => Reply::Permissions(permissions()),
            other => failed(format!("This Mac cannot do {other:?} yet.")),
        }
    }

    fn screenshot(&mut self, max_side: u32) -> Reply {
        if !CGPreflightScreenCaptureAccess() {
            return missing(Permission::ScreenRecording);
        }
        let bounds = CGDisplayBounds(CGMainDisplayID());
        let screen = bounds.size;
        let longest = screen.width.max(screen.height);
        if longest <= 0.0 {
            return failed("The main display has no size.");
        }
        let factor = (f64::from(max_side.clamp(320, 2560)) / longest).min(1.0);
        let width = pixels(screen.width * factor);
        let height = pixels(screen.height * factor);
        match capture(width, height) {
            Ok(jpeg) => {
                self.scale = screen.width / width as f64;
                Reply::Screenshot(Screenshot {
                    jpeg,
                    width: u32::try_from(width).unwrap_or(u32::MAX),
                    height: u32::try_from(height).unwrap_or(u32::MAX),
                    screen_width: pixels(screen.width) as u32,
                    screen_height: pixels(screen.height) as u32,
                })
            }
            Err(reason) => failed(reason),
        }
    }

    fn tree(&mut self, max_elements: u32) -> Reply {
        if let Some(no) = trusted() {
            return no;
        }
        // SAFETY: making the system-wide element has no preconditions.
        let system = unsafe { AXUIElement::new_system_wide() };
        // The system-wide focus can be unreadable, for one from a process
        // without windows; the workspace still knows the frontmost app.
        let front = NSWorkspace::sharedWorkspace().frontmostApplication();
        let app = attr(&system, "AXFocusedApplication")
            .and_then(as_element)
            .or_else(|| {
                let pid = front.as_ref()?.processIdentifier();
                // SAFETY: making an element for a pid has no preconditions.
                Some(unsafe { AXUIElement::new_application(pid) })
            });
        let Some(app) = app else {
            return failed("No app is in front.");
        };
        // SAFETY: `app` is a live element; the timeout covers its children.
        unsafe { app.set_messaging_timeout(AX_TIMEOUT) };
        let app_name = string_attr(&app, "AXTitle")
            .filter(|name| !name.is_empty())
            .or_else(|| Some(front.as_ref()?.localizedName()?.to_string()))
            .unwrap_or_default();
        let window = attr(&app, "AXFocusedWindow")
            .and_then(as_element)
            .or_else(|| children(&app, "AXWindows").into_iter().next());
        let title = window.as_ref().and_then(|w| string_attr(w, "AXTitle"));
        let root = window.unwrap_or_else(|| app.clone());
        let mut budget = MAX_READ;
        let node = read(root, 0, &mut budget);
        let compact = tree::compact(node, usize::try_from(max_elements).unwrap_or(usize::MAX));
        let elements = u32::try_from(compact.handles.len()).unwrap_or(u32::MAX);
        self.elements = compact.handles;
        Reply::Tree(Tree {
            app: app_name,
            window: title.filter(|t| !t.is_empty()),
            text: compact.text,
            elements,
            truncated: compact.truncated || budget == 0,
        })
    }

    fn click(&mut self, target: Target, button: MouseButton, count: u8) -> Reply {
        if let Some(no) = trusted() {
            return no;
        }
        let count = count.clamp(1, 3);
        if let Target::Element { index } = target {
            let Some(element) = self.element(index) else {
                return failed(format!("No element {index}; take a new tree."));
            };
            if element.pressable && button == MouseButton::Left && count == 1 {
                let press = CFString::from_static_str("AXPress");
                // SAFETY: the element and the action name are live CF objects.
                if unsafe { element.ax.perform_action(&press) } == AXError::Success {
                    return done(format!("Pressed [{index}] {}.", element.label));
                }
            }
            let Some(center) = element.center else {
                return failed(format!("Element {index} has no place on screen to click."));
            };
            let label = element.label.clone();
            mouse_click(center, button, count);
            return done(format!("Clicked [{index}] {label}."));
        }
        let Some(point) = self.point(target) else {
            return failed("That target is not on screen.");
        };
        mouse_click(point, button, count);
        done(format!("Clicked at {:.0},{:.0} points.", point.x, point.y))
    }

    fn scroll(&mut self, at: Option<Target>, dx: i32, dy: i32) -> Reply {
        if let Some(no) = trusted() {
            return no;
        }
        if let Some(target) = at {
            let Some(point) = self.point(target) else {
                return failed("That target is not on screen.");
            };
            post(CGEvent::new_mouse_event(
                None,
                CGEventType::MouseMoved,
                point,
                CGMouseButton::Left,
            ));
        }
        // A positive wheel moves the content down, which is scrolling up.
        post(CGEvent::new_scroll_wheel_event2(
            None,
            CGScrollEventUnit::Line,
            2,
            -dy.clamp(-100, 100),
            -dx.clamp(-100, 100),
            0,
        ));
        done("Scrolled.")
    }

    fn element(&self, index: u32) -> Option<&Element> {
        let at = usize::try_from(index).ok()?.checked_sub(1)?;
        self.elements.get(at)
    }

    /// A target in screen points.
    fn point(&self, target: Target) -> Option<CGPoint> {
        match target {
            Target::Element { index } => self.element(index)?.center,
            Target::Point { x, y } => Some(CGPoint::new(x * self.scale, y * self.scale)),
        }
    }
}

/// Which permissions are granted now.
pub fn permissions() -> Permissions {
    Permissions {
        screen_recording: CGPreflightScreenCaptureAccess(),
        // SAFETY: a plain query with no arguments.
        accessibility: unsafe { AXIsProcessTrusted() },
    }
}

/// Asks macOS for `permission` and opens its page in System Settings, where
/// the user turns Beep on.
pub fn request(permission: Permission) {
    let pane = match permission {
        Permission::ScreenRecording => {
            // Adds Beep to the list; macOS shows its own prompt once.
            CGRequestScreenCaptureAccess();
            "Privacy_ScreenCapture"
        }
        Permission::Accessibility => {
            let prompt = CFDictionary::from_slices(
                // SAFETY: the key is a constant CFString the framework exports.
                &[unsafe { kAXTrustedCheckOptionPrompt }],
                &[CFBoolean::new(true)],
            );
            // SAFETY: the options dictionary maps the prompt key to a boolean.
            unsafe { AXIsProcessTrustedWithOptions(Some(prompt.as_opaque())) };
            "Privacy_Accessibility"
        }
    };
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
    if let Err(e) = Command::new("open").arg(url).status() {
        log::warn!("opening System Settings: {e}");
    }
}

fn trusted() -> Option<Reply> {
    (!permissions().accessibility).then(|| missing(Permission::Accessibility))
}

fn done(detail: impl Into<String>) -> Reply {
    Reply::Done {
        detail: detail.into(),
    }
}

fn failed(reason: impl Into<String>) -> Reply {
    Reply::Failed {
        reason: reason.into(),
        missing: None,
    }
}

fn missing(permission: Permission) -> Reply {
    Reply::Failed {
        reason: format!(
            "{} is not allowed for Beep on this Mac.",
            permission.as_str()
        ),
        missing: Some(permission),
    }
}

/// Rounds a length in points to whole pixels, at least one.
fn pixels(points: f64) -> usize {
    // Display sizes are a few thousand points, far inside usize.
    points.round().max(1.0) as usize
}

/// The main display as a JPEG of `width` by `height` pixels.
fn capture(width: usize, height: usize) -> Result<Vec<u8>, String> {
    let (tx, rx) = sync_channel::<Result<Vec<u8>, String>>(1);
    let display_id = CGMainDisplayID();
    let on_content = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let started = (|| {
                // SAFETY: ScreenCaptureKit passes live content, or null with
                // an error, for the length of the call.
                let content = unsafe { content.as_ref() }.ok_or_else(|| error_text(error))?;
                // SAFETY: plain getters on live objects.
                let displays = unsafe { content.displays() };
                let display = displays
                    .iter()
                    .find(|display| unsafe { display.displayID() } == display_id)
                    .or_else(|| displays.firstObject())
                    .ok_or("This Mac has no display to capture.")?;
                // SAFETY: the display came from the content just read, and
                // the configuration only takes plain values.
                let (filter, config) = unsafe {
                    let filter = SCContentFilter::initWithDisplay_excludingWindows(
                        SCContentFilter::alloc(),
                        &display,
                        &NSArray::new(),
                    );
                    let config = SCStreamConfiguration::new();
                    config.setWidth(width);
                    config.setHeight(height);
                    config.setShowsCursor(true);
                    (filter, config)
                };
                let tx = tx.clone();
                let on_image = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
                    // SAFETY: the image is live, or null with an error, for
                    // the length of the call.
                    let result = match unsafe { image.as_ref() } {
                        Some(image) => jpeg(image),
                        None => Err(error_text(error)),
                    };
                    // The waiter may have given up already; then nobody
                    // wants the image.
                    let _ = tx.try_send(result);
                });
                // SAFETY: the filter and configuration are live, and the
                // handler only sends on a channel.
                unsafe {
                    SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                        &filter,
                        &config,
                        Some(&on_image),
                    );
                }
                Ok::<(), String>(())
            })();
            if let Err(reason) = started {
                // As above: a waiter that timed out no longer listens.
                let _ = tx.try_send(Err(reason));
            }
        },
    );
    // SAFETY: the handler is a valid block that outlives the call, as
    // ScreenCaptureKit copies it.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&on_content) };
    rx.recv_timeout(CAPTURE_TIMEOUT)
        .map_err(|_| "The screenshot took too long.".to_owned())?
}

fn error_text(error: *mut NSError) -> String {
    // SAFETY: the framework passes a live error or null.
    match unsafe { error.as_ref() } {
        Some(error) => format!("The screenshot failed: {}", error.localizedDescription()),
        None => "The screenshot failed.".into(),
    }
}

fn jpeg(image: &CGImage) -> Result<Vec<u8>, String> {
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), image);
    let quality = NSNumber::new_f64(QUALITY);
    let value: &AnyObject = &quality;
    // SAFETY: the key is a constant NSString AppKit exports.
    let key = unsafe { NSImageCompressionFactor };
    let properties = NSDictionary::from_slices(&[key], &[value]);
    // SAFETY: the properties map the compression key to a number, as the
    // JPEG encoder expects.
    let data =
        unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::JPEG, &properties) }
            .ok_or("The screenshot could not be encoded.")?;
    Ok(data.to_vec())
}

/// The value of `name` on `element`, when it has one.
fn attr(element: &AXUIElement, name: &str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_str(name);
    let mut value: *const CFType = ptr::null();
    // SAFETY: `value` is a valid out pointer; on success it holds a +1
    // reference we take over.
    let error = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut value)) };
    if error != AXError::Success {
        return None;
    }
    let value = NonNull::new(value.cast_mut())?;
    // SAFETY: the copy returned an owned reference, released on drop.
    Some(unsafe { CFRetained::from_raw(value) })
}

fn as_element(value: CFRetained<CFType>) -> Option<CFRetained<AXUIElement>> {
    value.downcast::<AXUIElement>().ok()
}

fn string_attr(element: &AXUIElement, name: &str) -> Option<String> {
    let value = attr(element, name)?;
    if let Some(text) = value.downcast_ref::<CFString>() {
        return Some(text.to_string());
    }
    if let Some(number) = value.downcast_ref::<CFNumber>() {
        return number.as_f64().map(|n| n.to_string());
    }
    None
}

fn bool_attr(element: &AXUIElement, name: &str) -> Option<bool> {
    attr(element, name)?
        .downcast_ref::<CFBoolean>()
        .map(CFBoolean::as_bool)
}

fn children(element: &AXUIElement, name: &str) -> Vec<CFRetained<AXUIElement>> {
    let Some(value) = attr(element, name) else {
        return Vec::new();
    };
    let Some(array) = value.downcast_ref::<CFArray>() else {
        return Vec::new();
    };
    // SAFETY: an accessibility array holds CF objects; each is checked
    // before use.
    let array = unsafe { array.cast_unchecked::<CFType>() };
    array.iter().filter_map(as_element).collect()
}

fn frame(element: &AXUIElement) -> Option<(CGPoint, CGSize)> {
    let position = attr(element, "AXPosition")?.downcast::<AXValue>().ok()?;
    let size = attr(element, "AXSize")?.downcast::<AXValue>().ok()?;
    let mut point = CGPoint::new(0.0, 0.0);
    let mut extent = CGSize::new(0.0, 0.0);
    // SAFETY: each out pointer matches the type asked for.
    let read = unsafe {
        position.value(AXValueType::CGPoint, NonNull::from(&mut point).cast())
            && size.value(AXValueType::CGSize, NonNull::from(&mut extent).cast())
    };
    read.then_some((point, extent))
}

fn pressable(element: &AXUIElement) -> bool {
    let mut names: *const CFArray = ptr::null();
    // SAFETY: `names` is a valid out pointer; on success it holds a +1 array.
    let error = unsafe { element.copy_action_names(NonNull::from(&mut names)) };
    let Some(names) = NonNull::new(names.cast_mut()).filter(|_| error == AXError::Success) else {
        return false;
    };
    // SAFETY: the copy returned an owned array of action name strings.
    let names = unsafe { CFRetained::from_raw(names) };
    let names = unsafe { names.cast_unchecked::<CFType>() };
    names.iter().any(|name| {
        name.downcast_ref::<CFString>()
            .is_some_and(|name| name.to_string() == "AXPress")
    })
}

/// Reads `ax` and its children, spending one of `budget` on each.
fn read(ax: CFRetained<AXUIElement>, depth: usize, budget: &mut usize) -> Node<Element> {
    *budget = budget.saturating_sub(1);
    let frame = frame(&ax);
    let role = string_attr(&ax, "AXRole").unwrap_or_default();
    let title = string_attr(&ax, "AXTitle");
    let description = string_attr(&ax, "AXDescription");
    let value = string_attr(&ax, "AXValue");
    let pressable = pressable(&ax);
    let kids = if depth < MAX_DEPTH && *budget > 0 {
        children(&ax, "AXChildren")
    } else {
        Vec::new()
    };
    let mut nodes = Vec::with_capacity(kids.len());
    for kid in kids {
        if *budget == 0 {
            break;
        }
        nodes.push(read(kid, depth + 1, budget));
    }
    let label = [&title, &description, &value]
        .into_iter()
        .flatten()
        .find(|text| !text.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| role.trim_start_matches("AX").to_owned());
    Node {
        enabled: bool_attr(&ax, "AXEnabled").unwrap_or(true),
        focused: bool_attr(&ax, "AXFocused").unwrap_or(false),
        visible: frame.is_some_and(|(_, size)| size.width > 0.0 && size.height > 0.0),
        pressable,
        handle: Element {
            center: frame.map(|(origin, size)| {
                CGPoint::new(origin.x + size.width / 2.0, origin.y + size.height / 2.0)
            }),
            pressable,
            label: format!("{label:?}"),
            ax,
        },
        role,
        title,
        description,
        value,
        children: nodes,
    }
}

fn post(event: Option<CFRetained<CGEvent>>) {
    CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
    sleep(EVENT_GAP);
}

fn mouse_click(point: CGPoint, button: MouseButton, count: u8) {
    let (down, up, which) = match button {
        MouseButton::Left => (
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGMouseButton::Left,
        ),
        MouseButton::Right => (
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGMouseButton::Right,
        ),
    };
    post(CGEvent::new_mouse_event(
        None,
        CGEventType::MouseMoved,
        point,
        CGMouseButton::Left,
    ));
    for click in 1..=count {
        for kind in [down, up] {
            let event = CGEvent::new_mouse_event(None, kind, point, which);
            CGEvent::set_integer_value_field(
                event.as_deref(),
                CGEventField::MouseEventClickState,
                i64::from(click),
            );
            post(event);
        }
    }
}

fn flags(modifiers: &[Modifier]) -> CGEventFlags {
    modifiers
        .iter()
        .fold(CGEventFlags::empty(), |flags, modifier| {
            flags
                | match modifier {
                    Modifier::Command => CGEventFlags::MaskCommand,
                    Modifier::Shift => CGEventFlags::MaskShift,
                    Modifier::Option => CGEventFlags::MaskAlternate,
                    Modifier::Control => CGEventFlags::MaskControl,
                    Modifier::Function => CGEventFlags::MaskSecondaryFn,
                }
        })
}

fn key(press: &KeyPress) -> Reply {
    let Some(code) = keys::code(&press.key) else {
        return failed(format!("{:?} is not a key this Mac knows.", press.key));
    };
    let mut flags = flags(&press.modifiers);
    if press.key == "+" {
        flags |= CGEventFlags::MaskShift;
    }
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(None, code, down);
        CGEvent::set_flags(event.as_deref(), flags);
        post(event);
    }
    done(format!("Pressed {press}."))
}

fn type_text(text: &str) -> Reply {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            for down in [true, false] {
                post(CGEvent::new_keyboard_event(None, 0x24, down));
            }
        }
        let units: Vec<u16> = line.encode_utf16().collect();
        for chunk in units.chunks(TYPE_CHUNK) {
            for down in [true, false] {
                let event = CGEvent::new_keyboard_event(None, 0, down);
                // SAFETY: the pointer and length describe `chunk`, which
                // outlives the call.
                unsafe {
                    CGEvent::keyboard_set_unicode_string(
                        event.as_deref(),
                        chunk.len() as _,
                        chunk.as_ptr(),
                    );
                }
                post(event);
            }
        }
    }
    done(format!("Typed {} characters.", text.chars().count()))
}

fn open(target: &str) -> Reply {
    let target = target.trim();
    if target.is_empty() {
        return failed("Open what? Name an app or a URL.");
    }
    let tries: &[&[&str]] = if target.contains("://") {
        &[&[]]
    } else if target.contains('.') && !target.contains(' ') {
        &[&["-a"], &["-b"]]
    } else {
        &[&["-a"]]
    };
    let mut last = String::new();
    for args in tries {
        match Command::new("open").args(*args).arg(target).output() {
            Ok(out) if out.status.success() => return done(format!("Opened {target}.")),
            Ok(out) => last = String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            Err(e) => last = e.to_string(),
        }
    }
    failed(format!("Could not open {target}: {last}"))
}

fn windows() -> Reply {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return failed("macOS did not list the windows.");
    };
    // SAFETY: the window list is an array of dictionaries keyed by strings.
    let list = unsafe { list.cast_unchecked::<CFDictionary>() };
    let mut windows = Vec::new();
    for info in list.iter() {
        // SAFETY: as above, the keys are CFStrings and values CF objects.
        let info = unsafe { info.cast_unchecked::<CFString, CFType>() };
        // SAFETY (each key): a constant CFString Core Graphics exports.
        let layer = unsafe { kCGWindowLayer };
        let owner = unsafe { kCGWindowOwnerName };
        let name = unsafe { kCGWindowName };
        let pid = unsafe { kCGWindowOwnerPID };
        let bounds = unsafe { kCGWindowBounds };
        let number = |key: &CFString| {
            info.get(key)
                .and_then(|v| v.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64))
        };
        let text = |key: &CFString| {
            info.get(key)
                .and_then(|v| v.downcast_ref::<CFString>().map(ToString::to_string))
        };
        // Layer 0 holds app windows; the rest are menus, the Dock and such.
        if number(layer) != Some(0) {
            continue;
        }
        let rect = info
            .get(bounds)
            .and_then(|v| v.downcast::<CFDictionary>().ok());
        let side = |key: &'static str| {
            rect.as_ref()
                .and_then(|rect| {
                    // SAFETY: a bounds dictionary maps strings to numbers.
                    let rect = unsafe { rect.cast_unchecked::<CFString, CFType>() };
                    rect.get(&CFString::from_static_str(key))
                })
                .and_then(|v| v.downcast_ref::<CFNumber>().and_then(CFNumber::as_f64))
                .map_or(0, |n| n.round() as i32)
        };
        windows.push(Window {
            app: text(owner).unwrap_or_default(),
            title: text(name).unwrap_or_default(),
            pid: number(pid).and_then(|p| i32::try_from(p).ok()).unwrap_or(0),
            x: side("X"),
            y: side("Y"),
            width: side("Width"),
            height: side("Height"),
        });
        if windows.len() >= MAX_WINDOWS {
            break;
        }
    }
    Reply::Windows { windows }
}
