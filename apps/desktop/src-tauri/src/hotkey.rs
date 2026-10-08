//! Double-tapping Option. An AppKit monitor for modifier changes sees
//! Option go down and up in any app, and macOS hands those out without the
//! Input Monitoring permission that watching every key would need.

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
mod monitor {
    use super::{DoubleTap, Input};
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
    use std::{cell::RefCell, ptr::NonNull, rc::Rc, time::Instant};

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
            Input::OptionUp
        }
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
