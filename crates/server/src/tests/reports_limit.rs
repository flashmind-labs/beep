use super::*;
use iroh::SecretKey;

fn monday() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 2).expect("a real date")
}

#[tokio::test]
async fn each_key_gets_its_own_count_and_a_new_day_starts_over() {
    let limit = KeyLimit::spawn(2).expect("inside a runtime");
    let a = SecretKey::generate().public();
    let b = SecretKey::generate().public();
    let tuesday = monday().succ_opt().expect("a next day");
    for expected in [true, true, false] {
        assert_eq!(
            limit.take(a, monday()).await.expect("the task runs"),
            expected
        );
    }
    assert!(limit.take(b, monday()).await.expect("the task runs"));
    assert!(limit.take(a, tuesday).await.expect("the task runs"));
}

#[tokio::test]
async fn a_report_given_back_can_be_sent_again() {
    let limit = KeyLimit::spawn(1).expect("inside a runtime");
    let key = SecretKey::generate().public();
    assert!(limit.take(key, monday()).await.expect("the task runs"));
    limit.give_back(key, monday()).await.expect("the task runs");
    assert!(limit.take(key, monday()).await.expect("the task runs"));
    assert!(!limit.take(key, monday()).await.expect("the task runs"));
}

#[tokio::test]
async fn past_the_day_s_keys_a_new_key_is_refused_until_the_next_day() {
    let limit = KeyLimit::spawn_with(5, 2).expect("inside a runtime");
    let keys: Vec<_> = (0..3).map(|_| SecretKey::generate().public()).collect();
    assert!(limit.take(keys[0], monday()).await.expect("the task runs"));
    assert!(limit.take(keys[1], monday()).await.expect("the task runs"));
    assert!(!limit.take(keys[2], monday()).await.expect("the task runs"));
    assert!(limit.take(keys[0], monday()).await.expect("the task runs"));
    let tuesday = monday().succ_opt().expect("a next day");
    assert!(limit.take(keys[2], tuesday).await.expect("the task runs"));
}

#[test]
fn outside_a_runtime_spawn_fails_instead_of_panicking() {
    assert!(KeyLimit::spawn(1).is_err());
}
