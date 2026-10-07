//! Double-tapping Option. A listen-only event tap sees every key and
//! modifier change, which needs the Input Monitoring permission.

use std::time::{Duration, Instant};

/// Longest a tap may be held, and longest between two taps.
const WINDOW: Duration = Duration::from_millis(300);

/// What the tap reports, stripped of Core Graphics types so the logic can
/// be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Option went down with no other modifier held.
    OptionDown,
    OptionUp,
    /// Any other key, modifier or click.
    Other,
}

/// Counts Option taps. Two quick taps in a row fire; anything else between
/// them, or a long hold, starts over.
#[derive(Debug, Default)]
pub struct DoubleTap {
    down_at: Option<Instant>,
    last_tap: Option<Instant>,
}

impl DoubleTap {
    /// True when `input` completes a double tap.
    pub fn on_event(&mut self, input: Input, now: Instant) -> bool {
        match input {
            Input::OptionDown => {
                self.down_at = Some(now);
                false
            }
            Input::OptionUp => {
                let Some(down) = self.down_at.take() else {
                    return false;
                };
                if now.duration_since(down) > WINDOW {
                    self.last_tap = None;
                    return false;
                }
                match self.last_tap.take() {
                    Some(last) if down.duration_since(last) <= WINDOW => true,
                    _ => {
                        self.last_tap = Some(now);
                        false
                    }
                }
            }
            Input::Other => {
                self.down_at = None;
                self.last_tap = None;
                false
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod tap {
    use super::{DoubleTap, Input};
    use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
    use core_graphics::event::{
        CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
        CGEventTapPlacement, CGEventType, EventField,
    };
    use std::{cell::RefCell, thread, time::Instant};

    const LEFT_OPTION: i64 = 58;
    const RIGHT_OPTION: i64 = 61;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
        fn CGRequestListenEventAccess() -> bool;
    }

    /// Whether the app may watch keys.
    pub fn allowed() -> bool {
        // SAFETY: takes no arguments and only reads the permission.
        unsafe { CGPreflightListenEventAccess() }
    }

    /// Asks for Input Monitoring. macOS shows its prompt once, and after
    /// that only System Settings can change it.
    pub fn request() -> bool {
        // SAFETY: takes no arguments.
        unsafe { CGRequestListenEventAccess() }
    }

    fn input(kind: CGEventType, event: &CGEvent) -> Input {
        if !matches!(kind, CGEventType::FlagsChanged) {
            return Input::Other;
        }
        let key = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
        if key != LEFT_OPTION && key != RIGHT_OPTION {
            return Input::Other;
        }
        let flags = event.get_flags();
        let others = CGEventFlags::CGEventFlagCommand
            | CGEventFlags::CGEventFlagControl
            | CGEventFlags::CGEventFlagShift
            | CGEventFlags::CGEventFlagSecondaryFn;
        if flags.intersects(others) {
            Input::Other
        } else if flags.contains(CGEventFlags::CGEventFlagAlternate) {
            Input::OptionDown
        } else {
            Input::OptionUp
        }
    }

    /// Starts the tap on its own thread and calls `fire` on each double tap.
    /// False if macOS refused the tap, which means no permission.
    pub fn start(fire: impl Fn() + Send + 'static) -> bool {
        let (ready, started) = std::sync::mpsc::channel();
        thread::Builder::new()
            .name("option-tap".into())
            .spawn(move || {
                let state = RefCell::new(DoubleTap::default());
                let tap = CGEventTap::new(
                    CGEventTapLocation::Session,
                    CGEventTapPlacement::TailAppendEventTap,
                    CGEventTapOptions::ListenOnly,
                    vec![
                        CGEventType::FlagsChanged,
                        CGEventType::KeyDown,
                        CGEventType::LeftMouseDown,
                        CGEventType::RightMouseDown,
                        CGEventType::OtherMouseDown,
                        CGEventType::ScrollWheel,
                    ],
                    |_, kind, event| {
                        if state
                            .borrow_mut()
                            .on_event(input(kind, event), Instant::now())
                        {
                            fire();
                        }
                        None
                    },
                );
                let Ok(tap) = tap else {
                    let _ = ready.send(false);
                    return;
                };
                let Ok(source) = tap.mach_port.create_runloop_source(0) else {
                    let _ = ready.send(false);
                    return;
                };
                // SAFETY: the source and the tap live until the run loop
                // below returns, which is never.
                unsafe { CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes) };
                tap.enable();
                let _ = ready.send(true);
                CFRunLoop::run_current();
            })
            .is_ok()
            && started.recv().unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
pub use tap::{allowed, request, start};

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    #[test]
    fn two_quick_taps_fire() {
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        assert!(!tap.on_event(Input::OptionDown, t));
        assert!(!tap.on_event(Input::OptionUp, ms(t, 80)));
        assert!(!tap.on_event(Input::OptionDown, ms(t, 200)));
        assert!(tap.on_event(Input::OptionUp, ms(t, 260)));
        // A third tap starts a new pair.
        assert!(!tap.on_event(Input::OptionDown, ms(t, 300)));
        assert!(!tap.on_event(Input::OptionUp, ms(t, 350)));
    }

    #[test]
    fn a_key_between_taps_resets() {
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(Input::OptionUp, ms(t, 50));
        tap.on_event(Input::Other, ms(t, 100));
        tap.on_event(Input::OptionDown, ms(t, 150));
        assert!(!tap.on_event(Input::OptionUp, ms(t, 200)));
    }

    #[test]
    fn slow_taps_and_long_holds_do_not_fire() {
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(Input::OptionUp, ms(t, 50));
        tap.on_event(Input::OptionDown, ms(t, 500));
        assert!(!tap.on_event(Input::OptionUp, ms(t, 550)));

        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(Input::OptionUp, ms(t, 50));
        tap.on_event(Input::OptionDown, ms(t, 100));
        assert!(!tap.on_event(Input::OptionUp, ms(t, 900)));
    }
}
