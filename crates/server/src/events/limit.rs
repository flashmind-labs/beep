//! Per-user limits on how many events are taken in, how often they are
//! triaged and how often they wake the agent. Times are passed in, so tests need no clock.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

/// The span both limits count over.
const SPAN: Duration = Duration::from_secs(60 * 60);

/// How many of something fit in an hour.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Events a user's accounts may store; past it they are dropped, so a
    /// flood of mail or a noisy repository cannot run up the database.
    pub events_per_hour: usize,
    /// Times the agent may be woken; past it events wait and go out
    /// together in the next wake.
    pub wakes_per_hour: usize,
    /// Triage runs a user's events may start; past it events wait and go
    /// out together in the next run.
    pub triages_per_hour: usize,
}

/// The moments something happened within the last span, oldest first.
#[derive(Debug, Default)]
struct Window(VecDeque<Instant>);

impl Window {
    /// Counts one more at `now` unless `max` already happened within the span.
    fn take(&mut self, now: Instant, max: usize) -> bool {
        while self
            .0
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= SPAN)
        {
            self.0.pop_front();
        }
        if self.0.len() >= max {
            return false;
        }
        self.0.push_back(now);
        true
    }

    /// Uncounts the newest one, taken for something that did not happen.
    fn give_back(&mut self) {
        self.0.pop_back();
    }
}

/// Both limits for every user, by user id.
#[derive(Debug)]
pub struct RateLimiter {
    limits: Limits,
    events: HashMap<String, Window>,
    wakes: HashMap<String, Window>,
    triages: HashMap<String, Window>,
}

impl RateLimiter {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            events: HashMap::new(),
            wakes: HashMap::new(),
            triages: HashMap::new(),
        }
    }

    /// Whether `user` may store one more event at `now`, counting it if so.
    pub fn take_event(&mut self, user: &str, now: Instant) -> bool {
        let max = self.limits.events_per_hour;
        self.events
            .entry(user.to_owned())
            .or_default()
            .take(now, max)
    }

    /// Uncounts an event taken for `user` that was not stored, such as a
    /// duplicate.
    pub fn give_back_event(&mut self, user: &str) {
        if let Some(window) = self.events.get_mut(user) {
            window.give_back();
        }
    }

    /// Uncounts a wake taken for `user` that started no run.
    pub fn give_back_wake(&mut self, user: &str) {
        if let Some(window) = self.wakes.get_mut(user) {
            window.give_back();
        }
    }

    /// Whether `user` may be woken at `now`, counting it if so.
    pub fn take_wake(&mut self, user: &str, now: Instant) -> bool {
        let max = self.limits.wakes_per_hour;
        self.wakes
            .entry(user.to_owned())
            .or_default()
            .take(now, max)
    }

    /// Whether `user` may start a triage run at `now`, counting it if so.
    pub fn take_triage(&mut self, user: &str, now: Instant) -> bool {
        let max = self.limits.triages_per_hour;
        self.triages
            .entry(user.to_owned())
            .or_default()
            .take(now, max)
    }
}

#[cfg(test)]
mod tests {
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
}
