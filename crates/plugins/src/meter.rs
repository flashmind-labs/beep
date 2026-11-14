use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::sandbox::locked;

/// The window the CPU figure covers.
pub(crate) const WINDOW: Duration = Duration::from_secs(60);
/// CPU time a plugin may use in one window before it is stopped.
pub(crate) const BUDGET: Duration = Duration::from_millis(500);
/// Entries closer together than this share one record, so the record stays short.
const MERGE: Duration = Duration::from_millis(250);

/// CPU time a plugin spent inside its script over the last minute, and its
/// pending timers. The window moves on the time the caller passes, so a test can
/// use its own clock; the amounts are CPU time measured by the sandbox.
pub(crate) struct Meter {
    budget: Duration,
    used: Mutex<VecDeque<(Instant, Duration)>>,
    timers: AtomicUsize,
}

impl Meter {
    pub(crate) fn new(budget: Duration) -> Self {
        Self {
            budget,
            used: Mutex::new(VecDeque::new()),
            timers: AtomicUsize::new(0),
        }
    }

    pub(crate) fn budget(&self) -> Duration {
        self.budget
    }

    /// Counts `spent` CPU time for an entry that ended at `now`.
    pub(crate) fn record(&self, now: Instant, spent: Duration) {
        let mut used = locked(&self.used);
        Self::expire(&mut used, now);

        match used.back_mut() {
            Some((at, total)) if now.saturating_duration_since(*at) < MERGE => {
                *total = total.saturating_add(spent);
            }
            _ => used.push_back((now, spent)),
        }
    }

    /// CPU time in the window that ends at `now`.
    pub(crate) fn used(&self, now: Instant) -> Duration {
        let mut used = locked(&self.used);
        Self::expire(&mut used, now);

        used.iter().fold(Duration::ZERO, |total, (_, spent)| {
            total.saturating_add(*spent)
        })
    }

    pub(crate) fn set_timers(&self, count: usize) {
        self.timers.store(count, Ordering::Relaxed);
    }

    pub(crate) fn timers(&self) -> usize {
        self.timers.load(Ordering::Relaxed)
    }

    fn expire(used: &mut VecDeque<(Instant, Duration)>, now: Instant) {
        while used
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= WINDOW)
        {
            used.pop_front();
        }
    }
}

#[cfg(test)]
#[path = "tests/meter.rs"]
mod tests;
