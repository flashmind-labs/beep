//! Tapping Option twice opens the bar, and one more tap with the bar open
//! starts live mode. An AppKit monitor for modifier changes sees
//! Option go down and up in any app, and macOS hands those out without the
//! Input Monitoring permission that watching every key would need. Keys and
//! clicks never reach that monitor, so when Option comes up we ask the event
//! source how long ago the last one was: Option used to type a character or
//! jump a word is not a tap.
//!
//! Only two taps open Beep. With the bar open, a single tap starts live
//! mode and holding Option talks until it is let go; in live mode a tap
//! interrupts Beep or leaves. Anywhere else Option alone does nothing, since
//! it is held for menus, Option-drag and Option-click. The same monitor
//! drives both, through `Hold`, and `on_single` and `on_held` decide.

#[cfg(desktop)]
use std::time::{Duration, Instant};

/// Longest a tap may be held, and longest between two taps.
#[cfg(desktop)]
pub const WINDOW: Duration = Duration::from_millis(300);

/// What the tap reports, stripped of Core Graphics types so the logic can
/// be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(desktop)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
#[cfg(desktop)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapStep {
    Nothing,
    /// Two taps: open or hide the bar.
    Two,
}

/// Counts quick Option taps in a row. Anything else between them, or a
/// long hold, starts over.
#[cfg(desktop)]
#[derive(Debug, Default)]
pub struct Taps {
    down_at: Option<Instant>,
    last_tap: Option<Instant>,
    count: u32,
}

#[cfg(desktop)]
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
                    self.forget();
                    return TapStep::Nothing;
                }
                let follows = self.last_tap.is_some_and(|last| {
                    down.duration_since(last) <= WINDOW && pressed.is_none_or(|p| p < last)
                });
                if !follows {
                    self.count = 1;
                    self.last_tap = Some(now);
                    return TapStep::Nothing;
                }
                // A third tap starts a run of its own: the single tap that
                // starts live mode once the bar is open.
                self.forget();
                TapStep::Two
            }
            Input::Other => {
                self.down_at = None;
                self.forget();
                TapStep::Nothing
            }
        }
    }

    fn forget(&mut self) {
        self.last_tap = None;
        self.count = 0;
    }
}

/// How long Option must be held alone before it counts as held. Longer
/// than a tap, so a double tap never talks.
#[cfg(desktop)]
pub const HOLD: Duration = Duration::from_millis(400);

/// What holding or tapping Option asks for.
#[cfg(desktop)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldStep {
    Nothing,
    /// Option went down: call `Hold::on_timer` with this press after `HOLD`.
    Wait(u64),
    /// Option is still held alone: listen while it is.
    Start,
    /// Option came up after a clean hold: send what was said.
    Send,
    /// Something else happened during the hold: throw it away.
    Cancel,
    /// Option was tapped once: call `Hold::on_tap_timer` with this press
    /// after `WINDOW`, once a double tap can no longer follow.
    Tap(u64),
    /// The tap stood alone.
    Single,
}

/// Tracks a hold of Option, and a tap that stands alone.
#[cfg(desktop)]
#[derive(Debug, Default)]
pub struct Hold {
    /// The press Option is held for, counted so a late timer from an older
    /// press does nothing.
    press: u64,
    held: bool,
    down_at: Option<Instant>,
    recording_since: Option<Instant>,
    /// When the last single tap ended, until its timer looks at it.
    tapped_at: Option<Instant>,
}

#[cfg(desktop)]
impl Hold {
    pub fn on_event(&mut self, input: Input, now: Instant) -> HoldStep {
        match input {
            Input::OptionDown => {
                self.press += 1;
                self.held = true;
                self.down_at = Some(now);
                self.recording_since = None;
                HoldStep::Wait(self.press)
            }
            Input::OptionUp { since_press } => {
                self.held = false;
                let down = self.down_at.take();
                // A key or click while Option was down means it was used for
                // something else, such as typing a character.
                let pressed = now.checked_sub(since_press);
                if let Some(since) = self.recording_since.take() {
                    return if pressed.is_some_and(|p| p >= since) {
                        HoldStep::Cancel
                    } else {
                        HoldStep::Send
                    };
                }
                let tap = down.is_some_and(|down| {
                    now.duration_since(down) <= WINDOW && pressed.is_none_or(|p| p < down)
                });
                if !tap {
                    return HoldStep::Nothing;
                }
                self.tapped_at = Some(now);
                HoldStep::Tap(self.press)
            }
            Input::Other => {
                self.held = false;
                self.down_at = None;
                self.tapped_at = None;
                if self.recording_since.take().is_some() {
                    HoldStep::Cancel
                } else {
                    HoldStep::Nothing
                }
            }
        }
    }

    /// `HOLD` after press `press`. `since_press` is how long ago a key or
    /// mouse button last went down: Option held to type a character or jump
    /// a word is not a hold.
    pub fn on_timer(&mut self, press: u64, since_press: Duration, now: Instant) -> HoldStep {
        if press != self.press || !self.held || self.recording_since.is_some() {
            return HoldStep::Nothing;
        }
        let pressed = now.checked_sub(since_press);
        if self
            .down_at
            .is_some_and(|down| pressed.is_some_and(|p| p >= down))
        {
            return HoldStep::Nothing;
        }
        self.recording_since = Some(now);
        HoldStep::Start
    }

    /// `WINDOW` after the tap of press `press`. `since_press` is how long
    /// ago a key or mouse button last went down: typing right after the tap
    /// means Option was part of something else.
    pub fn on_tap_timer(&mut self, press: u64, since_press: Duration, now: Instant) -> HoldStep {
        let Some(tapped) = self.tapped_at else {
            return HoldStep::Nothing;
        };
        if press != self.press || self.held {
            return HoldStep::Nothing;
        }
        self.tapped_at = None;
        if now.checked_sub(since_press).is_some_and(|p| p >= tapped) {
            return HoldStep::Nothing;
        }
        HoldStep::Single
    }

    /// Whether a tap waits for `on_tap_timer`.
    pub fn tapped(&self) -> bool {
        self.tapped_at.is_some()
    }

    /// The last tap was part of two or three, so it does not stand alone.
    pub fn forget_tap(&mut self) {
        self.tapped_at = None;
    }
}

/// What live mode is doing, as Option sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Off,
    /// Live mode the user started from the bar.
    Live,
    /// A voice note being read out, which does not listen.
    Note,
}

/// What a lone tap or a hold of Option does.
#[cfg(desktop)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Nothing,
    /// Start live mode in the chat the bar shows.
    Start,
    /// Hand the key to live mode: a tap interrupts or leaves, a hold talks.
    Live,
    /// Stop the voice note.
    EndNote,
}

/// What two taps of Option do.
#[cfg(desktop)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Two {
    /// Open the bar, or hide it.
    Toggle,
    /// Bring the bar back over the orb, live mode still running.
    ShowBar,
    /// Leave live mode, or stop the voice note, and open the bar on the chat.
    Leave,
}

/// Two taps. `bar_open` is whether the bar is on screen.
#[cfg(desktop)]
pub fn on_two(voice: Voice, bar_open: bool) -> Two {
    match voice {
        Voice::Off => Two::Toggle,
        Voice::Live if !bar_open => Two::ShowBar,
        Voice::Live | Voice::Note => Two::Leave,
    }
}

/// A lone tap. `bar_open` is a bar the user opened with two taps.
#[cfg(desktop)]
pub fn on_single(voice: Voice, bar_open: bool) -> Act {
    match voice {
        Voice::Off if bar_open => Act::Start,
        Voice::Off => Act::Nothing,
        Voice::Live => Act::Live,
        Voice::Note => Act::EndNote,
    }
}

/// Option held alone. `bar_open` is a bar the user opened with two taps.
#[cfg(desktop)]
pub fn on_held(voice: Voice, bar_open: bool) -> Act {
    match voice {
        Voice::Off if bar_open => Act::Start,
        Voice::Live => Act::Live,
        // A note never starts listening on its own.
        Voice::Off | Voice::Note => Act::Nothing,
    }
}

#[cfg(target_os = "macos")]
mod monitor {
    use super::Input;
    use block2::RcBlock;
    use objc2::{rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
    use std::{cell::RefCell, ptr::NonNull, rc::Rc, time::Duration};

    const LEFT_OPTION: u16 = 58;
    const RIGHT_OPTION: u16 = 61;
    const ESCAPE: u16 = 53;

    thread_local! {
        /// The Escape monitors while live mode runs. Main thread only.
        static ESCAPE_MONITORS: RefCell<Vec<Retained<AnyObject>>> = const { RefCell::new(Vec::new()) };
    }

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
    pub fn since_press() -> Duration {
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

    /// Calls `on` when Escape goes down in one of Beep's windows, until
    /// `unwatch_escape`. Escape in another app never reaches it. `on` says
    /// whether it handled the key; an unhandled Escape goes on to the
    /// window. Call on the main thread.
    pub fn watch_escape(on: impl Fn() -> bool + 'static) {
        let local = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event for the length of the call.
            if unsafe { event.as_ref() }.keyCode() == ESCAPE && on() {
                // Handled: the bar would otherwise act on it again.
                return std::ptr::null_mut();
            }
            event.as_ptr()
        });
        // SAFETY: the block returns the event it was given or nothing.
        let local = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &local)
        };
        let old = ESCAPE_MONITORS.with(|m| m.replace(local.into_iter().collect()));
        for monitor in old {
            remove(&monitor);
        }
    }

    /// Stops watching Escape. Call on the main thread.
    pub fn unwatch_escape() {
        for monitor in ESCAPE_MONITORS.with(RefCell::take) {
            remove(&monitor);
        }
    }

    fn remove(monitor: &AnyObject) {
        // SAFETY: `monitor` is a token `addGlobalMonitorForEventsMatchingMask`
        // returned and that has not been removed yet.
        unsafe { NSEvent::removeMonitor(monitor) };
    }
}

#[cfg(target_os = "macos")]
pub use monitor::{since_press, start, unwatch_escape, watch_escape};

#[cfg(all(desktop, not(target_os = "macos")))]
pub fn start(_: impl Fn(Input) + 'static) {}

#[cfg(all(desktop, not(target_os = "macos")))]
pub fn since_press() -> Duration {
    Duration::MAX
}

#[cfg(not(target_os = "macos"))]
pub fn watch_escape(_: impl Fn() -> bool + 'static) {}

#[cfg(not(target_os = "macos"))]
pub fn unwatch_escape() {}

#[cfg(all(test, desktop))]
#[path = "../tests/voice_hotkey.rs"]
mod tests;
