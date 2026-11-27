//! Tapping Option twice opens or hides the bar. An AppKit monitor for
//! modifier changes sees Option go down and up in any app, and macOS hands
//! those out without the Input Monitoring permission that watching every key
//! would need. Keys and clicks never reach that monitor, so when Option comes
//! up we ask the event source how long ago the last one was: Option used to
//! type a character or jump a word is not a tap. Anything else Option does
//! is left alone, since it is held for menus, Option-drag and Option-click.
// Only macOS has the monitor; elsewhere the taps are tested but unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::time::{Duration, Instant};
use tauri::AppHandle;
#[cfg(target_os = "macos")]
use {crate::beep::state::Prefs, tauri::Manager};

/// Longest a tap may be held, and longest between two taps.
pub const WINDOW: Duration = Duration::from_millis(300);

/// What the monitor reports, stripped of AppKit types so the logic can be
/// tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Option went down with no other modifier held.
    OptionDown,
    /// Option came up. `since_press` is how long ago a key or mouse button
    /// last went down, in any app.
    OptionUp { since_press: Duration },
    /// Any other key, modifier or click.
    Other,
}

/// What a run of quick Option taps asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapStep {
    Nothing,
    /// Two taps: open or hide the bar.
    Two,
}

/// Counts quick Option taps in a row. Anything else between them, or a
/// long hold, starts over.
#[derive(Debug, Default)]
pub struct Taps {
    down_at: Option<Instant>,
    last_tap: Option<Instant>,
}

impl Taps {
    pub fn on_event(&mut self, input: Input, now: Instant) -> TapStep {
        match input {
            Input::OptionDown => {
                self.down_at = Some(now);
                TapStep::Nothing
            }
            Input::OptionUp { since_press } => {
                let Some(down) = self.down_at.take() else {
                    return TapStep::Nothing;
                };
                // No press at all since the clock started counts as long ago.
                let pressed = now.checked_sub(since_press);

                if now.duration_since(down) > WINDOW || pressed.is_some_and(|p| p >= down) {
                    self.last_tap = None;
                    return TapStep::Nothing;
                }

                let follows = self.last_tap.is_some_and(|last| {
                    down.duration_since(last) <= WINDOW && pressed.is_none_or(|p| p < last)
                });

                if !follows {
                    self.last_tap = Some(now);
                    return TapStep::Nothing;
                }
                // A third tap starts a run of its own.
                self.last_tap = None;
                TapStep::Two
            }
            Input::Other => {
                self.down_at = None;
                self.last_tap = None;
                TapStep::Nothing
            }
        }
    }
}

/// Toggles the bar on two taps of Option, for as long as the app runs,
/// unless turned off in Settings.
#[cfg(target_os = "macos")]
pub fn start(app: AppHandle) {
    let taps = std::cell::RefCell::new(Taps::default());
    monitor::start(move |input| {
        if taps.borrow_mut().on_event(input, Instant::now()) == TapStep::Two
            && app.state::<Prefs>().option_taps()
        {
            super::toggle(&app);
        }
    });
}

/// Option is a macOS key: elsewhere the bar opens from its shortcut.
#[cfg(not(target_os = "macos"))]
pub fn start(_: AppHandle) {}

#[cfg(target_os = "macos")]
mod monitor {
    use super::Input;
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
    use std::{ptr::NonNull, rc::Rc, time::Duration};

    const LEFT_OPTION: u16 = 58;
    const RIGHT_OPTION: u16 = 61;

    fn input(event: &NSEvent) -> Input {
        let key = event.keyCode();

        if key != LEFT_OPTION && key != RIGHT_OPTION {
            return Input::Other;
        }

        let flags = event.modifierFlags();
        let others = NSEventModifierFlags::Command
            | NSEventModifierFlags::Control
            | NSEventModifierFlags::Shift
            | NSEventModifierFlags::Function;
        if flags.intersects(others) {
            Input::Other
        } else if flags.contains(NSEventModifierFlags::Option) {
            Input::OptionDown
        } else {
            Input::OptionUp {
                since_press: since_press(),
            }
        }
    }

    /// How long ago a key or mouse button last went down, in any app.
    fn since_press() -> Duration {
        [
            CGEventType::KeyDown,
            CGEventType::LeftMouseDown,
            CGEventType::RightMouseDown,
            CGEventType::OtherMouseDown,
        ]
        .into_iter()
        .map(|kind| {
            let secs = CGEventSource::seconds_since_last_event_type(
                CGEventSourceStateID::CombinedSessionState,
                kind,
            );
            Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)
        })
        .min()
        .unwrap_or(Duration::MAX)
    }

    /// Watches modifier changes in other apps and in our own windows, and
    /// hands each to `each`. Runs on the main thread, which is where AppKit
    /// delivers the events.
    pub fn start(each: impl Fn(Input) + 'static) {
        let on_event = Rc::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit passes a live event for the length of the call.
            let event = unsafe { event.as_ref() };
            each(input(event));
        });

        let global = on_event.clone();
        let global = RcBlock::new(move |event: NonNull<NSEvent>| global(event));
        let local = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            on_event(event);
            // Handing the event back passes it on; returning nothing would
            // swallow every modifier change in our own windows.
            event.as_ptr()
        });
        let global = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::FlagsChanged,
            &global,
        );
        // SAFETY: the block always returns the event it was given.
        let local = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::FlagsChanged, &local)
        };

        if global.is_none() {
            log::warn!("macOS refused the Option monitor");
        }
        // The monitors stop when their tokens drop, and they should run for
        // as long as the app does.
        std::mem::forget(global);
        std::mem::forget(local);
    }
}

#[cfg(test)]
#[path = "../tests/bar_hotkey.rs"]
mod tests;
