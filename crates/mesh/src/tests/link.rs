use super::{Beat, MISSED};

#[test]
fn a_link_is_given_up_after_missed_beats_in_a_row() {
    let mut beat = Beat::new(10);

    for _ in 1..MISSED {
        assert!(!beat.quiet(10));
    }

    assert!(beat.quiet(10));
}

#[test]
fn a_packet_between_beats_starts_the_count_again() {
    let mut beat = Beat::new(10);

    for _ in 1..MISSED {
        assert!(!beat.quiet(10));
    }

    assert!(!beat.quiet(11));

    for _ in 1..MISSED {
        assert!(!beat.quiet(11));
    }

    assert!(beat.quiet(11));
}
