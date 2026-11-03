//! When a project's copy of its host lists is read again. Every decision
//! takes `now`, so the tests drive the clock.

use std::time::{Duration, Instant};

/// How old a copy may get before an ask, or the background sync, reads the
/// host again.
pub(crate) const FRESH: Duration = Duration::from_secs(180);
/// As `FRESH`, after the last read failed.
pub(crate) const FRESH_AFTER_ERROR: Duration = Duration::from_secs(60);
/// How long after the last ask the background sync keeps a project fresh.
pub(crate) const VIEWED_FOR: Duration = Duration::from_secs(30 * 60);
/// The shortest time between two reads Refresh forces.
pub(crate) const FORCE_EVERY: Duration = Duration::from_secs(30);
/// How long after an ask the project page counts as open, for live
/// progress. The page asks about every 10 seconds while a run is going.
pub(crate) const OPEN_FOR: Duration = Duration::from_secs(30);
/// The shortest time between two reads of a running run's jobs.
pub(crate) const POLL_EVERY: Duration = Duration::from_secs(10);
/// The shortest time between two reads webhooks ask for, so a burst of
/// events (a CI run's jobs) reads the host once or twice.
pub(crate) const HOOK_GAP: Duration = Duration::from_secs(5);
/// A project nobody asked about for this long is dropped from memory.
pub(crate) const FORGET_AFTER: Duration = Duration::from_secs(2 * 60 * 60);

/// What an ask does about the host before it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AskRead {
    /// The copy is fresh: it is the answer.
    Copy,
    /// The copy is old: it is the answer as it is, and the background sync
    /// reads the host and tells the pages following the project.
    CopyThenSync,
    /// Read the host first: there is no copy yet, or Refresh may read now.
    Host,
}

/// One copy's asks and reads.
#[derive(Debug, Default, Clone)]
pub(crate) struct Timing {
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
    pub(crate) fn view(&mut self, now: Instant) {
        self.viewed = Some(now);
    }

    /// The host was read, successfully or not.
    pub(crate) fn synced(&mut self, now: Instant, ok: bool) {
        self.synced = Some(now);
        self.failed = !ok;
        self.poked = false;
    }

    /// A webhook said the host changed: the copy is read again once it is
    /// `HOOK_GAP` old.
    pub(crate) fn poke(&mut self) {
        self.poked = true;
    }

    /// Whether the copy is missing or old enough to read again.
    pub(crate) fn stale(&self, now: Instant) -> bool {
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

    /// What an ask does: `forced` when Refresh may read now, `has_copy`
    /// once a read succeeded. An old copy never keeps the page waiting on
    /// the host.
    pub(crate) fn ask_read(&self, now: Instant, forced: bool, has_copy: bool) -> AskRead {
        if forced || (!has_copy && self.stale(now)) {
            AskRead::Host
        } else if self.stale(now) {
            AskRead::CopyThenSync
        } else {
            AskRead::Copy
        }
    }

    /// Whether a Refresh may read the host now; records it when it may.
    pub(crate) fn force(&mut self, now: Instant) -> bool {
        if recent(self.forced, now, FORCE_EVERY) {
            return false;
        }
        self.forced = Some(now);
        true
    }

    /// Whether the background sync reads it: someone asked in the last
    /// half hour and the copy is old.
    pub(crate) fn background_due(&self, now: Instant) -> bool {
        recent(self.viewed, now, VIEWED_FOR) && self.stale(now)
    }

    /// Whether the page counts as open now.
    pub(crate) fn open(&self, now: Instant) -> bool {
        recent(self.viewed, now, OPEN_FOR)
    }

    /// Whether running jobs may be read now: the page is open and the last
    /// poll is `POLL_EVERY` old. Records the poll when they may.
    pub(crate) fn poll(&mut self, now: Instant) -> bool {
        if !self.open(now) || recent(self.polled, now, POLL_EVERY) {
            return false;
        }
        self.polled = Some(now);
        true
    }

    /// Whether nobody asked for so long that the copy can go.
    pub(crate) fn forgotten(&self, now: Instant) -> bool {
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
    fn a_copy_is_stale_until_read_then_after_three_minutes() {
        let start = Instant::now();
        let mut t = Timing::default();
        assert!(t.stale(start), "never read");
        t.synced(start, true);
        assert!(!t.stale(start + secs(179)));
        assert!(t.stale(start + secs(180)));
    }

    #[test]
    fn a_failed_read_is_tried_again_after_a_minute() {
        let start = Instant::now();
        let mut t = Timing::default();
        t.synced(start, false);
        assert!(!t.stale(start + secs(59)));
        assert!(t.stale(start + secs(60)));
    }

    #[test]
    fn the_background_syncs_only_projects_viewed_in_the_last_half_hour() {
        let start = Instant::now();
        let mut t = Timing::default();
        t.synced(start, true);
        assert!(
            !t.background_due(start + secs(600)),
            "never viewed, never synced in the background"
        );
        t.view(start);
        assert!(!t.background_due(start + secs(60)), "still fresh");
        assert!(t.background_due(start + secs(181)));
        t.synced(start + secs(181), true);
        assert!(!t.background_due(start + secs(200)));
        assert!(t.background_due(start + secs(29 * 60 + 59)));
        assert!(
            !t.background_due(start + secs(30 * 60)),
            "nobody looked for half an hour"
        );
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
    fn refresh_reads_at_most_once_per_thirty_seconds() {
        let start = Instant::now();
        let mut t = Timing::default();
        assert!(t.force(start));
        assert!(!t.force(start + secs(10)));
        assert!(!t.force(start + secs(29)));
        assert!(t.force(start + secs(30)));
        assert!(!t.force(start + secs(31)));
    }

    #[test]
    fn jobs_are_polled_every_ten_seconds_while_the_page_is_open() {
        let start = Instant::now();
        let mut t = Timing::default();
        assert!(!t.poll(start), "nobody has the page open");
        t.view(start);
        assert!(t.poll(start));
        // A second viewer asking within the ten seconds shares the read.
        t.view(start + secs(4));
        assert!(!t.poll(start + secs(4)));
        assert!(t.poll(start + secs(10)));
        // The page closed: no asks for half a minute, no polls.
        assert!(!t.poll(start + secs(40)));
        t.view(start + secs(41));
        assert!(t.poll(start + secs(41)));
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

    #[test]
    fn a_project_nobody_asked_about_for_two_hours_is_forgotten() {
        let start = Instant::now();
        let mut t = Timing::default();
        t.view(start);
        assert!(!t.forgotten(start + secs(7199)));
        assert!(t.forgotten(start + secs(7200)));
        assert!(Timing::default().forgotten(start));
    }
}
