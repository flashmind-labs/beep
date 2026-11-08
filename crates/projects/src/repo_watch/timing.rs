//! When a project's copy of its host lists is read again. Decisions take `now` so tests drive the clock.

use std::time::{Duration, Instant};

/// How old a copy may get before an ask or the background sync rereads the host.
pub const FRESH: Duration = Duration::from_secs(180);
/// As `FRESH`, after the last read failed.
pub const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);
/// How long after the last ask the background sync keeps a project fresh.
pub const VIEWED_FOR: Duration = Duration::from_secs(30 * 60);
/// The shortest time between two reads Refresh forces.
pub const FORCE_EVERY: Duration = Duration::from_secs(30);
/// How long after an ask the page counts as open; it asks about every 10 seconds during a run.
pub const OPEN_FOR: Duration = Duration::from_secs(30);
/// The shortest time between two reads of a running run's jobs.
pub const POLL_EVERY: Duration = Duration::from_secs(10);
/// A project nobody asked about for this long is dropped from memory.
pub const FORGET_AFTER: Duration = Duration::from_secs(2 * 60 * 60);

/// What an ask does about the host before it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskRead {
    /// The copy is fresh: it is the answer.
    Copy,
    /// The copy is old: answer with it, and the background sync rereads and notifies followers.
    CopyThenSync,
    /// Read the host first: there is no copy yet, or Refresh may read now.
    Host,
}

/// One copy's asks and reads.
#[derive(Debug, Default, Clone)]
pub struct Timing {
    viewed: Option<Instant>,
    synced: Option<Instant>,
    failed: bool,
    forced: Option<Instant>,
    polled: Option<Instant>,
}

/// Whether `then` is less than `within` before `now`.
fn recent(then: Option<Instant>, now: Instant, within: Duration) -> bool {
    then.is_some_and(|then| now.saturating_duration_since(then) < within)
}

impl Timing {
    /// Someone asked for the copy.
    pub fn view(&mut self, now: Instant) {
        self.viewed = Some(now);
    }

    /// The host was read, successfully or not.
    pub fn synced(&mut self, now: Instant, ok: bool) {
        self.synced = Some(now);
        self.failed = !ok;
    }

    /// Whether the copy is missing or old enough to read again.
    pub fn stale(&self, now: Instant) -> bool {
        let ttl = if self.failed {
            FRESH_AFTER_ERROR
        } else {
            FRESH
        };
        !recent(self.synced, now, ttl)
    }

    /// What an ask does; `forced` when Refresh may read now, `has_copy` once a read succeeded.
    pub fn ask_read(&self, now: Instant, forced: bool, has_copy: bool) -> AskRead {
        if forced || (!has_copy && self.stale(now)) {
            AskRead::Host
        } else if self.stale(now) {
            AskRead::CopyThenSync
        } else {
            AskRead::Copy
        }
    }

    /// Whether a Refresh may read the host now; records it when it may.
    pub fn force(&mut self, now: Instant) -> bool {
        if recent(self.forced, now, FORCE_EVERY) {
            return false;
        }
        self.forced = Some(now);
        true
    }

    /// Whether the background sync reads it: asked in the last half hour and the copy is old.
    pub fn background_due(&self, now: Instant) -> bool {
        recent(self.viewed, now, VIEWED_FOR) && self.stale(now)
    }

    /// Whether the page counts as open now.
    pub fn open(&self, now: Instant) -> bool {
        recent(self.viewed, now, OPEN_FOR)
    }

    /// Whether running jobs may be read now (page open, last poll `POLL_EVERY` old); records the poll.
    pub fn poll(&mut self, now: Instant) -> bool {
        if !self.open(now) || recent(self.polled, now, POLL_EVERY) {
            return false;
        }
        self.polled = Some(now);
        true
    }

    /// Whether nobody asked for so long that the copy can go.
    pub fn forgotten(&self, now: Instant) -> bool {
        !recent(self.viewed, now, FORGET_AFTER)
    }
}

#[cfg(test)]
#[path = "../tests/repo_watch_timing.rs"]
mod tests;
