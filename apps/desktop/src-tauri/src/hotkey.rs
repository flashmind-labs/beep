//! Tapping Option: twice opens the bar, three times starts live mode. An AppKit monitor for modifier changes sees
//! Option go down and up in any app, and macOS hands those out without the
//! Input Monitoring permission that watching every key would need. Keys and
//! clicks never reach that monitor, so when Option comes up we ask the event
//! source how long ago the last one was: Option used to type a character or
//! jump a word is not a tap.
//!
//! Holding Option on its own talks to Beep in live mode, and in live mode
//! a single tap interrupts Beep or leaves. The same monitor drives both, through
//! `Hold`.

use std::time::{Duration, Instant};

/// Longest a tap may be held, and longest between two taps.
pub const WINDOW: Duration = Duration::from_millis(300);

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

/// What a run of quick Option taps asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapStep {
    Nothing,
    /// Two taps: call `Taps::on_timer` with this run after `WINDOW`, once a
    /// third can no longer follow. Then it opens or hides the bar.
    Two(u64),
    /// Three taps: live mode.
    Three,
}

/// Counts quick Option taps in a row. Anything else between them, or a
/// long hold, starts over.
#[derive(Debug, Default)]
pub struct Taps {
    down_at: Option<Instant>,
    last_tap: Option<Instant>,
    count: u32,
    /// Counted, so a timer from an earlier run does nothing.
    run: u64,
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
                    self.forget();
                    return TapStep::Nothing;
                }
                let follows = self.last_tap.is_some_and(|last| {
                    down.duration_since(last) <= WINDOW && pressed.is_none_or(|p| p < last)
                });
                if follows {
                    self.count += 1;
                } else {
                    self.count = 1;
                    self.run += 1;
                }
                self.last_tap = Some(now);
                match self.count {
                    2 => TapStep::Two(self.run),
                    3 => {
                        self.forget();
                        TapStep::Three
                    }
                    _ => TapStep::Nothing,
                }
            }
            Input::Other => {
                self.down_at = None;
                self.forget();
                TapStep::Nothing
            }
        }
    }

    /// `WINDOW` after the second tap of `run`: true when no third came, so
    /// the two taps stand.
    pub fn on_timer(&mut self, run: u64) -> bool {
        if run != self.run || self.count != 2 || self.down_at.is_some() {
            return false;
        }
        self.forget();
        true
    }

    fn forget(&mut self) {
        self.last_tap = None;
        self.count = 0;
    }
}

/// How long Option must be held alone before it counts as held. Longer
/// than a tap, so a double tap never talks.
pub const HOLD: Duration = Duration::from_millis(400);

/// What holding or tapping Option asks for.
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

    /// `HOLD` after press `press`. `armed` says whether holding talks now.
    pub fn on_timer(&mut self, press: u64, armed: bool, now: Instant) -> HoldStep {
        if press != self.press || !self.held || !armed || self.recording_since.is_some() {
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

    /// The last tap was part of two or three, so it does not stand alone.
    pub fn forget_tap(&mut self) {
        self.tapped_at = None;
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
        /// The Escape monitor while live mode runs. Main thread only.
        static ESCAPE_MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
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

    /// Calls `on` when Escape goes down in another app, until `unwatch_escape`.
    /// Other apps' keys only reach Beep with the Accessibility permission;
    /// without it this sees nothing, and Escape works in the bar alone.
    /// Call on the main thread.
    pub fn watch_escape(on: impl Fn() + 'static) {
        let block = RcBlock::new(move |event: NonNull<NSEvent>| {
            // SAFETY: AppKit passes a live event for the length of the call.
            let event = unsafe { event.as_ref() };
            if event.keyCode() == ESCAPE {
                on();
            }
        });
        let monitor =
            NSEvent::addGlobalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block);
        let old = ESCAPE_MONITOR.with(|m| m.replace(monitor));
        if let Some(old) = old {
            remove(&old);
        }
    }

    /// Stops watching Escape. Call on the main thread.
    pub fn unwatch_escape() {
        if let Some(monitor) = ESCAPE_MONITOR.with(RefCell::take) {
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

    /// Taps at each start time, 60 ms long.
    fn taps(t: &mut Taps, base: Instant, starts: &[u64]) -> Vec<TapStep> {
        starts
            .iter()
            .map(|&at| {
                t.on_event(Input::OptionDown, ms(base, at));
                t.on_event(UP, ms(base, at + 60))
            })
            .collect()
    }

    #[test]
    fn two_quick_taps_wait_for_a_third_then_stand() {
        let t = Instant::now();
        let mut tap = Taps::default();
        let steps = taps(&mut tap, t, &[0, 200]);
        let TapStep::Two(run) = steps[1] else {
            panic!("two taps should wait for a third");
        };
        assert_eq!(steps[0], TapStep::Nothing);
        assert!(tap.on_timer(run));
        // It fires once.
        assert!(!tap.on_timer(run));
    }

    #[test]
    fn three_quick_taps_are_live_mode_and_not_two() {
        let t = Instant::now();
        let mut tap = Taps::default();
        let steps = taps(&mut tap, t, &[0, 200, 400]);
        let TapStep::Two(run) = steps[1] else {
            panic!("two taps should wait for a third");
        };
        assert_eq!(steps[2], TapStep::Three);
        assert!(!tap.on_timer(run));
        // A fourth tap starts over.
        assert_eq!(taps(&mut tap, t, &[600]), vec![TapStep::Nothing]);
    }

    #[test]
    fn a_third_tap_still_down_at_the_timer_holds_off_the_two() {
        let t = Instant::now();
        let mut tap = Taps::default();
        let TapStep::Two(run) = taps(&mut tap, t, &[0, 200])[1] else {
            panic!("two taps should wait for a third");
        };
        tap.on_event(Input::OptionDown, ms(t, 500));
        assert!(!tap.on_timer(run));
        assert_eq!(tap.on_event(UP, ms(t, 560)), TapStep::Three);
    }

    #[test]
    fn a_key_between_taps_resets() {
        let t = Instant::now();
        let mut tap = Taps::default();
        taps(&mut tap, t, &[0]);
        tap.on_event(Input::Other, ms(t, 100));
        assert_eq!(taps(&mut tap, t, &[150]), vec![TapStep::Nothing]);
    }

    #[test]
    fn slow_taps_and_long_holds_do_not_count() {
        let t = Instant::now();
        let mut tap = Taps::default();
        assert_eq!(
            taps(&mut tap, t, &[0, 500]),
            vec![TapStep::Nothing, TapStep::Nothing]
        );

        let mut tap = Taps::default();
        taps(&mut tap, t, &[0]);
        tap.on_event(Input::OptionDown, ms(t, 100));
        assert_eq!(tap.on_event(UP, ms(t, 900)), TapStep::Nothing);
    }

    #[test]
    fn option_typing_a_character_twice_does_not_count() {
        // Option-2 then Option-2 again, as when typing "@@" on a Spanish
        // keyboard: each key went down while Option was held.
        let t = Instant::now();
        let mut tap = Taps::default();
        tap.on_event(Input::OptionDown, t);
        assert_eq!(
            tap.on_event(up_after_press(30), ms(t, 80)),
            TapStep::Nothing
        );
        tap.on_event(Input::OptionDown, ms(t, 150));
        assert_eq!(
            tap.on_event(up_after_press(30), ms(t, 230)),
            TapStep::Nothing
        );
    }

    #[test]
    fn a_plain_key_between_taps_resets() {
        let t = Instant::now();
        let mut tap = Taps::default();
        taps(&mut tap, t, &[0]);
        tap.on_event(Input::OptionDown, ms(t, 200));
        // A key went down at 120, after the first tap and before the second.
        assert_eq!(
            tap.on_event(up_after_press(130), ms(t, 250)),
            TapStep::Nothing
        );
    }

    #[test]
    fn holding_option_records_and_letting_go_sends() {
        let t = Instant::now();
        let mut hold = Hold::default();
        let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, t) else {
            panic!("a press should wait for the timer");
        };
        assert_eq!(hold.on_timer(press, true, ms(t, 400)), HoldStep::Start);
        assert_eq!(
            hold.on_event(up_after_press(5000), ms(t, 2000)),
            HoldStep::Send
        );
    }

    #[test]
    fn a_short_press_or_an_unarmed_hold_records_nothing() {
        let t = Instant::now();
        let mut hold = Hold::default();
        let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, t) else {
            panic!("a press should wait for the timer");
        };
        // A short press is a tap instead.
        assert_eq!(hold.on_event(UP, ms(t, 100)), HoldStep::Tap(press));
        assert_eq!(hold.on_timer(press, true, ms(t, 400)), HoldStep::Nothing);

        let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, ms(t, 1000)) else {
            panic!("a press should wait for the timer");
        };
        assert_eq!(hold.on_timer(press, false, ms(t, 1400)), HoldStep::Nothing);
        assert_eq!(hold.on_event(UP, ms(t, 2000)), HoldStep::Nothing);
    }

    #[test]
    fn a_late_timer_from_an_earlier_press_does_nothing() {
        let t = Instant::now();
        let mut hold = Hold::default();
        let HoldStep::Wait(first) = hold.on_event(Input::OptionDown, t) else {
            panic!("a press should wait for the timer");
        };
        hold.on_event(UP, ms(t, 100));
        hold.on_event(Input::OptionDown, ms(t, 300));
        assert_eq!(hold.on_timer(first, true, ms(t, 400)), HoldStep::Nothing);
    }

    #[test]
    fn a_key_or_modifier_during_a_recording_cancels_it() {
        let t = Instant::now();
        let mut hold = Hold::default();
        let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, t) else {
            panic!("a press should wait for the timer");
        };
        hold.on_timer(press, true, ms(t, 400));
        // A key went down at 900, during the recording.
        assert_eq!(
            hold.on_event(up_after_press(100), ms(t, 1000)),
            HoldStep::Cancel
        );

        let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, ms(t, 2000)) else {
            panic!("a press should wait for the timer");
        };
        hold.on_timer(press, true, ms(t, 2400));
        assert_eq!(hold.on_event(Input::Other, ms(t, 2500)), HoldStep::Cancel);
        assert_eq!(hold.on_event(UP, ms(t, 2600)), HoldStep::Nothing);
    }

    #[test]
    fn a_hold_is_not_a_tap_in_a_run() {
        let t = Instant::now();
        let mut tap = Taps::default();
        taps(&mut tap, t, &[0]);
        tap.on_event(Input::OptionDown, ms(t, 150));
        assert_eq!(tap.on_event(UP, ms(t, 150) + HOLD), TapStep::Nothing);
    }

    #[test]
    fn a_key_before_the_first_tap_does_not_stop_the_run() {
        let t = ms(Instant::now(), 1000);
        let mut tap = Taps::default();
        tap.on_event(Input::OptionDown, t);
        tap.on_event(up_after_press(100), ms(t, 50));
        tap.on_event(Input::OptionDown, ms(t, 150));
        assert!(matches!(
            tap.on_event(up_after_press(300), ms(t, 200)),
            TapStep::Two(_)
        ));
    }

    fn tap(hold: &mut Hold, at: Instant) -> HoldStep {
        hold.on_event(Input::OptionDown, at);
        hold.on_event(UP, at + Duration::from_millis(80))
    }

    #[test]
    fn a_lone_tap_is_single_and_holds_still_work_after() {
        let t = ms(Instant::now(), 1000);
        let mut hold = Hold::default();
        let HoldStep::Tap(press) = tap(&mut hold, t) else {
            panic!("a tap should wait for a second one");
        };
        assert_eq!(
            hold.on_tap_timer(press, Duration::MAX, ms(t, 400)),
            HoldStep::Single
        );
        // The timer fires once.
        assert_eq!(
            hold.on_tap_timer(press, Duration::MAX, ms(t, 450)),
            HoldStep::Nothing
        );
        let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, ms(t, 2000)) else {
            panic!("a press should wait for the timer");
        };
        assert_eq!(hold.on_timer(press, true, ms(t, 2400)), HoldStep::Start);
        assert_eq!(hold.on_event(UP, ms(t, 3000)), HoldStep::Send);
        let HoldStep::Tap(press) = tap(&mut hold, ms(t, 4000)) else {
            panic!("a tap should wait for a second one");
        };
        assert_eq!(
            hold.on_tap_timer(press, Duration::MAX, ms(t, 4400)),
            HoldStep::Single
        );
    }

    #[test]
    fn a_double_tap_is_not_single() {
        let t = ms(Instant::now(), 1000);
        let mut hold = Hold::default();
        let HoldStep::Tap(first) = tap(&mut hold, t) else {
            panic!("a tap should wait for a second one");
        };
        tap(&mut hold, ms(t, 200));
        assert_eq!(
            hold.on_tap_timer(first, Duration::MAX, ms(t, 400)),
            HoldStep::Nothing
        );
    }

    #[test]
    fn a_tap_that_typed_is_not_single() {
        let t = ms(Instant::now(), 1000);
        let mut hold = Hold::default();
        // A key went down 100 ms before the timer, after the tap.
        let HoldStep::Tap(press) = tap(&mut hold, t) else {
            panic!("a tap should wait for a second one");
        };
        assert_eq!(
            hold.on_tap_timer(press, Duration::from_millis(100), ms(t, 400)),
            HoldStep::Nothing
        );
        // Option used to type a character is not a tap.
        hold.on_event(Input::OptionDown, ms(t, 2000));
        assert_eq!(
            hold.on_event(up_after_press(30), ms(t, 2080)),
            HoldStep::Nothing
        );
    }

    #[test]
    fn a_hold_is_not_a_tap() {
        let t = ms(Instant::now(), 1000);
        let mut hold = Hold::default();
        let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, t) else {
            panic!("a press should wait for the timer");
        };
        assert_eq!(hold.on_timer(press, false, ms(t, 400)), HoldStep::Nothing);
        assert_eq!(hold.on_event(UP, ms(t, 900)), HoldStep::Nothing);
    }
}
