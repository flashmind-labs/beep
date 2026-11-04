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
/// The shortest gap between webhook-driven reads, so a burst of events reads once or twice.
pub const HOOK_GAP: Duration = Duration::from_secs(5);
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
    /// A webhook said the host changed since the last read.
    poked: bool,
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
        self.poked = false;
    }

    /// A webhook said the host changed: reread once the copy is `HOOK_GAP` old.
    pub fn poke(&mut self) {
        self.poked = true;
    }

    /// Whether the copy is missing or old enough to read again.
    pub fn stale(&self, now: Instant) -> bool {
        if self.poked && !recent(self.synced, now, HOOK_GAP) {
            return true;
        }
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
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn an_old_copy_answers_at_once_and_is_read_in_the_background() {
        let start = Instant::now();
        let mut t = Timing::default();
        assert_eq!(
            t.ask_read(start, false, false),
            AskRead::Host,
            "no copy yet"
        );
        t.synced(start, true);
        assert_eq!(t.ask_read(start + secs(60), false, true), AskRead::Copy);
        assert_eq!(
            t.ask_read(start + secs(180), false, true),
            AskRead::CopyThenSync
        );
        assert_eq!(
            t.ask_read(start + secs(180), true, true),
            AskRead::Host,
            "Refresh reads now"
        );
        // Every read failed so far: there is nothing to show but the host.
        let mut failed = Timing::default();
        failed.synced(start, false);
        assert_eq!(
            failed.ask_read(start + secs(60), false, false),
            AskRead::Host
        );
        assert_eq!(
            failed.ask_read(start + secs(30), false, false),
            AskRead::Copy,
            "tried a moment ago"
        );
    }

    #[test]
    fn a_webhook_reads_again_within_seconds_but_not_more_than_every_five() {
        let start = Instant::now();
        let mut t = Timing::default();
        t.synced(start, true);
        t.poke();
        assert!(!t.stale(start + secs(4)), "read a moment ago");
        assert!(t.stale(start + secs(5)));
        t.synced(start + secs(5), true);
        assert!(!t.stale(start + secs(60)), "the read took the poke");
    }
}
