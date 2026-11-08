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
fn two_quick_taps_open_the_bar_at_once() {
    let t = Instant::now();
    let mut tap = Taps::default();
    assert_eq!(
        taps(&mut tap, t, &[0, 200]),
        vec![TapStep::Nothing, TapStep::Two]
    );
}

#[test]
fn a_third_tap_starts_over() {
    let t = Instant::now();
    let mut tap = Taps::default();
    assert_eq!(
        taps(&mut tap, t, &[0, 200, 400, 600]),
        vec![
            TapStep::Nothing,
            TapStep::Two,
            TapStep::Nothing,
            TapStep::Two
        ]
    );
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
    assert_eq!(
        hold.on_timer(press, Duration::MAX, ms(t, 400)),
        HoldStep::Start
    );
    assert_eq!(
        hold.on_event(up_after_press(5000), ms(t, 2000)),
        HoldStep::Send
    );
}

#[test]
fn a_short_press_records_nothing() {
    let t = Instant::now();
    let mut hold = Hold::default();
    let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, t) else {
        panic!("a press should wait for the timer");
    };
    // A short press is a tap instead.
    assert_eq!(hold.on_event(UP, ms(t, 100)), HoldStep::Tap(press));
    assert_eq!(
        hold.on_timer(press, Duration::MAX, ms(t, 400)),
        HoldStep::Nothing
    );
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
    assert_eq!(
        hold.on_timer(first, Duration::MAX, ms(t, 400)),
        HoldStep::Nothing
    );
}

#[test]
fn a_key_or_modifier_during_a_recording_cancels_it() {
    let t = Instant::now();
    let mut hold = Hold::default();
    let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, t) else {
        panic!("a press should wait for the timer");
    };
    hold.on_timer(press, Duration::MAX, ms(t, 400));
    // A key went down at 900, during the recording.
    assert_eq!(
        hold.on_event(up_after_press(100), ms(t, 1000)),
        HoldStep::Cancel
    );

    let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, ms(t, 2000)) else {
        panic!("a press should wait for the timer");
    };
    hold.on_timer(press, Duration::MAX, ms(t, 2400));
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
    assert_eq!(tap.on_event(up_after_press(300), ms(t, 200)), TapStep::Two);
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
    assert_eq!(
        hold.on_timer(press, Duration::MAX, ms(t, 2400)),
        HoldStep::Start
    );
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
fn a_tap_after_a_double_tap_is_single() {
    let t = ms(Instant::now(), 1000);
    let mut hold = Hold::default();
    tap(&mut hold, t);
    tap(&mut hold, ms(t, 200));
    let HoldStep::Tap(third) = tap(&mut hold, ms(t, 400)) else {
        panic!("a third tap should be a tap of its own");
    };
    assert_eq!(
        hold.on_tap_timer(third, Duration::MAX, ms(t, 800)),
        HoldStep::Single
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
fn option_held_to_type_or_jump_words_is_not_a_hold() {
    let t = ms(Instant::now(), 1000);
    let mut hold = Hold::default();
    let HoldStep::Wait(press) = hold.on_event(Input::OptionDown, t) else {
        panic!("a press should wait for the timer");
    };
    // A key went down 100 ms after Option, 300 ms before the timer.
    assert_eq!(
        hold.on_timer(press, Duration::from_millis(300), ms(t, 400)),
        HoldStep::Nothing
    );
    assert_eq!(
        hold.on_event(up_after_press(50), ms(t, 600)),
        HoldStep::Nothing
    );
}

#[test]
fn option_alone_starts_nothing_without_the_bar() {
    assert_eq!(on_single(Voice::Off, false), Act::Nothing);
    assert_eq!(on_held(Voice::Off, false), Act::Nothing);
    assert_eq!(on_single(Voice::Off, true), Act::Start);
    assert_eq!(on_held(Voice::Off, true), Act::Start);
}

#[test]
fn live_mode_takes_taps_and_holds_with_the_bar_hidden() {
    assert_eq!(on_single(Voice::Live, false), Act::Live);
    assert_eq!(on_held(Voice::Live, false), Act::Live);
}

#[test]
fn a_voice_note_never_starts_listening() {
    assert_eq!(on_single(Voice::Note, false), Act::EndNote);
    assert_eq!(on_held(Voice::Note, false), Act::Nothing);
    assert_eq!(on_held(Voice::Note, true), Act::Nothing);
}

#[test]
fn two_taps_bring_back_the_bar_over_the_orb() {
    assert_eq!(on_two(Voice::Off, false), Two::Toggle);
    assert_eq!(on_two(Voice::Off, true), Two::Toggle);
    assert_eq!(on_two(Voice::Live, false), Two::ShowBar);
    assert_eq!(on_two(Voice::Live, true), Two::Leave);
    assert_eq!(on_two(Voice::Note, false), Two::Leave);
}

#[test]
fn a_hold_is_not_a_tap() {
    let t = ms(Instant::now(), 1000);
    let mut hold = Hold::default();
    let HoldStep::Wait(_) = hold.on_event(Input::OptionDown, t) else {
        panic!("a press should wait for the timer");
    };
    assert_eq!(hold.on_event(UP, ms(t, 900)), HoldStep::Nothing);
}
