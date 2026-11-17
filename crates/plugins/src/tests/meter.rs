use super::*;

fn ms(count: u64) -> Duration {
    Duration::from_millis(count)
}

#[test]
fn time_counts_for_a_minute_then_leaves_the_window() {
    let start = Instant::now();
    let meter = Meter::new(BUDGET);

    meter.record(start, ms(100));
    meter.record(start + Duration::from_secs(30), ms(50));

    assert_eq!(meter.used(start + Duration::from_secs(30)), ms(150));
    assert_eq!(meter.used(start + Duration::from_secs(59)), ms(150));
    assert_eq!(meter.used(start + Duration::from_secs(60)), ms(50));
    assert_eq!(meter.used(start + Duration::from_secs(90)), ms(0));
}

#[test]
fn close_entries_share_a_record() {
    let start = Instant::now();
    let meter = Meter::new(BUDGET);

    for step in 0..1000 {
        meter.record(start + Duration::from_millis(step), ms(1));
    }

    assert_eq!(meter.used(start + Duration::from_secs(2)), ms(1000));
    assert!(locked(&meter.used).len() <= 5);
}
