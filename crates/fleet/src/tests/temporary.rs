use super::*;

fn temporary(list: &[(u8, bool)]) -> HashMap<u8, bool> {
    list.iter().copied().collect()
}

const DAY: u64 = 24 * 60 * 60;

#[test]
fn a_temporary_device_offline_for_a_day_is_due() {
    let mut leftovers = Leftovers::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    assert!(leftovers.step(&temporary(&[(1, true)]), at(0)).is_empty());
    assert!(leftovers.step(&temporary(&[(1, false)]), at(10)).is_empty());
    assert!(
        leftovers
            .step(&temporary(&[(1, false)]), at(10 + DAY - 1))
            .is_empty()
    );
    assert_eq!(leftovers.step(&temporary(&[(1, false)]), at(10 + DAY)), [1]);
}

#[test]
fn coming_back_starts_the_day_over() {
    let mut leftovers = Leftovers::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    leftovers.step(&temporary(&[(1, false)]), at(0));
    assert!(
        leftovers
            .step(&temporary(&[(1, true)]), at(DAY - 100))
            .is_empty()
    );
    assert!(
        leftovers
            .step(&temporary(&[(1, false)]), at(DAY))
            .is_empty()
    );
    assert!(
        leftovers
            .step(&temporary(&[(1, false)]), at(2 * DAY - 1))
            .is_empty()
    );
    assert_eq!(leftovers.step(&temporary(&[(1, false)]), at(2 * DAY)), [1]);
}

#[test]
fn a_device_off_at_start_counts_from_then_and_one_gone_is_forgotten() {
    let mut leftovers = Leftovers::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    assert!(leftovers.step(&temporary(&[(1, false)]), at(0)).is_empty());
    // Removed by another desktop: no longer listed.
    assert!(leftovers.step(&temporary(&[]), at(300)).is_empty());
    // Listed again (a new device with the same key cannot be, but the list may lag).
    assert!(
        leftovers
            .step(&temporary(&[(1, false)]), at(600))
            .is_empty()
    );
    assert_eq!(
        leftovers.step(&temporary(&[(1, false)]), at(600 + DAY)),
        [1]
    );
}

#[test]
fn the_sweep_wakes_when_the_first_device_off_is_due() {
    let mut leftovers = Leftovers::default();
    let start = Instant::now();
    let at = |secs| start + Duration::from_secs(secs);
    assert_eq!(leftovers.due(), None, "nothing is off");

    leftovers.step(&temporary(&[(1, false), (2, true)]), at(10));
    leftovers.step(&temporary(&[(1, false), (2, false)]), at(50));
    assert_eq!(leftovers.due(), Some(at(10) + GONE_AFTER));

    leftovers.step(&temporary(&[(1, true), (2, false)]), at(60));
    assert_eq!(leftovers.due(), Some(at(50) + GONE_AFTER));
}
