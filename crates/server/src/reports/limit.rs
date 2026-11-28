//! How many reports each key may send in a UTC day.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::NaiveDate;
use iroh_base::EndpointId;

/// Keys counted in a day. Keys cost nothing to make, so past this a key not
/// yet counted is refused for the rest of the day.
const MAX_KEYS: usize = 10_000;

pub(crate) struct KeyLimit {
    per_day: u32,
    max_keys: usize,
    state: Mutex<State>,
}

struct State {
    counts: HashMap<EndpointId, Count>,
    today: NaiveDate,
}

struct Count {
    day: NaiveDate,
    sent: u32,
}

impl KeyLimit {
    pub(crate) fn new(per_day: u32) -> Self {
        Self::with_max_keys(per_day, MAX_KEYS)
    }

    fn with_max_keys(per_day: u32, max_keys: usize) -> Self {
        Self {
            per_day,
            max_keys,
            state: Mutex::new(State {
                counts: HashMap::new(),
                today: NaiveDate::MIN,
            }),
        }
    }

    /// Uses one of `key`'s reports for `day`; false when none are left.
    pub(crate) fn take(&self, key: EndpointId, day: NaiveDate) -> bool {
        let mut state = self.lock();

        if day > state.today {
            state.today = day;
            state.counts.retain(|_, count| count.day >= day);
        }
        if state.counts.len() >= self.max_keys && !state.counts.contains_key(&key) {
            return false;
        }

        let count = state.counts.entry(key).or_insert(Count { day, sent: 0 });

        if count.day != day {
            *count = Count { day, sent: 0 };
        }

        let allowed = count.sent < self.per_day;
        if allowed {
            count.sent += 1;
        }

        allowed
    }

    /// Returns the report `take` used for `day`, for one the store then
    /// failed to keep.
    pub(crate) fn give_back(&self, key: EndpointId, day: NaiveDate) {
        if let Some(count) = self.lock().counts.get_mut(&key)
            && count.day == day
        {
            count.sent = count.sent.saturating_sub(1);
        }
    }

    /// The counts stay consistent between operations, so a poisoned lock is
    /// still usable.
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
#[path = "../tests/reports_limit.rs"]
mod tests;
