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
fn a_hold_is_not_a_tap_in_a_run() {
    let t = Instant::now();
    let mut tap = Taps::default();
    taps(&mut tap, t, &[0]);
    tap.on_event(Input::OptionDown, ms(t, 150));
    assert_eq!(tap.on_event(UP, ms(t, 550)), TapStep::Nothing);
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
