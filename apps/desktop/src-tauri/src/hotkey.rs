//! Double-tapping Option. An AppKit monitor for modifier changes sees
//! Option go down and up in any app, and macOS hands those out without the
//! Input Monitoring permission that watching every key would need. Keys and
//! clicks never reach that monitor, so when Option comes up we ask the event
//! source how long ago the last one was: Option used to type a character or
//! jump a word is not a tap.

use std::time::{Duration, Instant};

/// Longest a tap may be held, and longest between two taps.
const WINDOW: Duration = Duration::from_millis(300);

/// What the tap reports, stripped of Core Graphics types so the logic can
/// be tested.
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
            Input::OptionUp { since_press } => {
                let Some(down) = self.down_at.take() else {
                    return false;
                };
                // No press at all since the clock started counts as long ago.
                let pressed = now.checked_sub(since_press);
                if now.duration_since(down) > WINDOW || pressed.is_some_and(|p| p >= down) {
                    self.last_tap = None;
                    return false;
                }
                match self.last_tap.take() {
                    Some(last)
                        if down.duration_since(last) <= WINDOW
                            && pressed.is_none_or(|p| p < last) =>
                    {
                        true
                    }
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
mod monitor {
    use super::{DoubleTap, Input};
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
    use std::{
        cell::RefCell,
        ptr::NonNull,
        rc::Rc,
        time::{Duration, Instant},
    };

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
    /// calls `fire` on each double tap. Runs on the main thread, which is
    /// where AppKit delivers the events.
    pub fn start(fire: impl Fn() + 'static) {
        let state = RefCell::new(DoubleTap::default());
        let on_event = Rc::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit passes a live event for the length of the call.
            let event = unsafe { event.as_ref() };
            if state.borrow_mut().on_event(input(event), Instant::now()) {
                fire();
            }
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

#[cfg(target_os = "macos")]
pub use monitor::start;

#[cfg(test)]
mod tests {
    use super::*;

    /// Option up with no key or click for as long as the clock goes back.
    const UP: Input = Input::OptionUp {
        since_press: Duration::MAX,
    };

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    fn up_after_press(ms: u64) -> Input {
        Input::OptionUp {
            since_press: Duration::from_millis(ms),
        }
    }

    #[test]
    fn two_quick_taps_fire() {
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        assert!(!tap.on_event(Input::OptionDown, t));
        assert!(!tap.on_event(UP, ms(t, 80)));
        assert!(!tap.on_event(Input::OptionDown, ms(t, 200)));
        assert!(tap.on_event(UP, ms(t, 260)));
        // A third tap starts a new pair.
        assert!(!tap.on_event(Input::OptionDown, ms(t, 300)));
        assert!(!tap.on_event(UP, ms(t, 350)));
    }

    #[test]
    fn a_key_between_taps_resets() {
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(UP, ms(t, 50));
        tap.on_event(Input::Other, ms(t, 100));
        tap.on_event(Input::OptionDown, ms(t, 150));
        assert!(!tap.on_event(UP, ms(t, 200)));
    }

    #[test]
    fn slow_taps_and_long_holds_do_not_fire() {
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(UP, ms(t, 50));
        tap.on_event(Input::OptionDown, ms(t, 500));
        assert!(!tap.on_event(UP, ms(t, 550)));

        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(UP, ms(t, 50));
        tap.on_event(Input::OptionDown, ms(t, 100));
        assert!(!tap.on_event(UP, ms(t, 900)));
    }

    #[test]
    fn option_typing_a_character_twice_does_not_fire() {
        // Option-2 then Option-2 again, as when typing "@@" on a Spanish
        // keyboard: each key went down while Option was held.
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        assert!(!tap.on_event(up_after_press(30), ms(t, 80)));
        tap.on_event(Input::OptionDown, ms(t, 150));
        assert!(!tap.on_event(up_after_press(30), ms(t, 230)));
    }

    #[test]
    fn a_plain_key_between_taps_resets() {
        let t = Instant::now();
        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(UP, ms(t, 50));
        tap.on_event(Input::OptionDown, ms(t, 200));
        // A key went down at 120, after the first tap and before the second.
        assert!(!tap.on_event(up_after_press(130), ms(t, 250)));
    }

    #[test]
    fn a_key_before_the_first_tap_does_not_stop_it() {
        let t = ms(Instant::now(), 1000);
        let mut tap = DoubleTap::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(up_after_press(100), ms(t, 50));
        tap.on_event(Input::OptionDown, ms(t, 150));
        assert!(tap.on_event(up_after_press(300), ms(t, 200)));
    }
}
