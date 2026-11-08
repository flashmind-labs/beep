use super::*;

fn limiter() -> RateLimiter {
    RateLimiter::new(Limits {
        events_per_hour: 3,
        wakes_per_hour: 2,
        triages_per_hour: 1,
    })
}

#[test]
fn wakes_stop_at_the_limit_and_come_back_after_an_hour() {
    let mut limiter = limiter();
    let start = Instant::now();
    assert!(limiter.take_wake("alice", start));
    assert!(limiter.take_wake("alice", start + Duration::from_secs(60)));
    assert!(!limiter.take_wake("alice", start + Duration::from_secs(120)));
    // A refused wake is not counted, so the first one aging out frees one.
    assert!(limiter.take_wake("alice", start + SPAN));
    assert!(!limiter.take_wake("alice", start + SPAN + Duration::from_secs(1)));
    assert!(limiter.take_wake("alice", start + SPAN + Duration::from_secs(60)));
}

#[test]
fn what_did_not_happen_is_given_back() {
    let mut limiter = limiter();
    let now = Instant::now();
    for _ in 0..3 {
        assert!(limiter.take_event("alice", now));
        limiter.give_back_event("alice");
    }
    assert!(limiter.take_event("alice", now), "duplicates do not count");
    assert!(limiter.take_wake("alice", now));
    assert!(limiter.take_wake("alice", now));
    limiter.give_back_wake("alice");
    assert!(
        limiter.take_wake("alice", now),
        "a wake with no run is free"
    );
    assert!(!limiter.take_wake("alice", now));
}

#[test]
fn each_user_and_each_limit_counts_on_its_own() {
    let mut limiter = limiter();
    let now = Instant::now();
    for _ in 0..3 {
        assert!(limiter.take_event("alice", now));
    }
    assert!(!limiter.take_event("alice", now));
    assert!(limiter.take_event("bob", now));
    assert!(
        limiter.take_wake("alice", now),
        "events do not use up wakes"
    );
    assert!(limiter.take_triage("alice", now));
    assert!(!limiter.take_triage("alice", now));
    assert!(limiter.take_triage("bob", now));
    assert!(
        limiter.take_wake("alice", now),
        "triages do not use up wakes"
    );
}
