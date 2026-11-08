use super::*;

#[test]
fn a_slow_trip_is_dropped() {
    assert_eq!(clock_offset_ms(0, MAX_ROUND_TRIP_MS + 1, 0), None);
    assert_eq!(clock_offset_ms(0, MAX_ROUND_TRIP_MS, 1_000), Some(0));
}
