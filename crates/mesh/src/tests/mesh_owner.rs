use super::{Backoff, FIRST_RETRY, LAST_RETRY};

#[test]
fn an_absent_member_is_tried_about_once_a_minute() {
    let mut backoff = Backoff(FIRST_RETRY);
    let waits: Vec<_> = (0..12).map(|_| backoff.pause()).collect();

    assert!(waits[0] <= FIRST_RETRY.mul_f64(1.25));
    assert!(waits.iter().all(|w| *w <= LAST_RETRY.mul_f64(1.25)));
    assert!(waits[11] >= LAST_RETRY.mul_f64(0.75));

    backoff.reset();
    assert!(backoff.pause() <= FIRST_RETRY.mul_f64(1.25));
}
