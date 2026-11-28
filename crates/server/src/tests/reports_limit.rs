use super::*;
use iroh_base::SecretKey;

fn monday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 2).expect("a real date")
}

#[test]
fn each_key_gets_its_own_count_and_a_new_day_starts_over() {
    let limit = KeyLimit::new(2);
    let a = SecretKey::generate().public();
    let b = SecretKey::generate().public();
    let tuesday = monday().succ_opt().expect("a next day");

    for expected in [true, true, false] {
        assert_eq!(limit.take(a, monday()), expected);
    }

    assert!(limit.take(b, monday()));
    assert!(limit.take(a, tuesday));
}

#[test]
fn a_report_given_back_can_be_sent_again() {
    let limit = KeyLimit::new(1);
    let key = SecretKey::generate().public();
    assert!(limit.take(key, monday()));
    limit.give_back(key, monday());
    assert!(limit.take(key, monday()));
    assert!(!limit.take(key, monday()));
}

#[test]
fn past_the_day_s_keys_a_new_key_is_refused_until_the_next_day() {
    let limit = KeyLimit::with_max_keys(5, 2);
    let keys: Vec<_> = (0..3).map(|_| SecretKey::generate().public()).collect();
    assert!(limit.take(keys[0], monday()));
    assert!(limit.take(keys[1], monday()));
    assert!(!limit.take(keys[2], monday()));
    assert!(limit.take(keys[0], monday()));
    let tuesday = monday().succ_opt().expect("a next day");
    assert!(limit.take(keys[2], tuesday));
}
