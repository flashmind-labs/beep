//! When the search index reads a session: once it has been quiet for a
//! while, not again until it has new activity, and later after each
//! failure. Change notifications only start a pass; they cannot skip a wait.

use beep_proto::SessionId;
use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};
use tokio::{
    sync::Notify,
    time::{Instant, sleep_until},
};

/// At most one pass starts in this time, however often the sessions change.
pub(super) const PASS_GAP: Duration = Duration::from_secs(60);
/// A session is read once it has had no activity for this long: until
/// then its last reply may still grow.
pub(super) const QUIET: Duration = Duration::from_secs(60);
/// A session that could not be read is tried again after this, then less
/// often up to `RETRY_MOST`.
const RETRY_FIRST: Duration = Duration::from_secs(30);
const RETRY_MOST: Duration = Duration::from_secs(600);

/// What the indexer knows of each session in this run, by identity.
#[derive(Default)]
pub(super) struct Schedule(HashMap<SessionId, Tracked>);

#[derive(Default)]
struct Tracked {
    /// The last activity the session was found fully indexed at.
    indexed: Option<SystemTime>,
    failures: u32,
    retry_at: Option<Instant>,
}

/// Whether a pass reads a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Due {
    /// Indexed at its last activity.
    Current,
    /// Not before this.
    At(Instant),
    Now,
}

impl Schedule {
    /// `idle` is how long ago the session's `activity` was.
    pub fn due(&self, id: &SessionId, activity: SystemTime, idle: Duration, now: Instant) -> Due {
        let tracked = self.0.get(id);

        if tracked.is_some_and(|t| t.indexed == Some(activity)) {
            return Due::Current;
        }

        if let Some(at) = tracked.and_then(|t| t.retry_at).filter(|at| *at > now) {
            return Due::At(at);
        }

        if idle < QUIET {
            return Due::At(now + (QUIET - idle));
        }

        Due::Now
    }

    pub fn indexed(&mut self, id: SessionId, activity: SystemTime) {
        self.0.insert(
            id,
            Tracked {
                indexed: Some(activity),
                ..Tracked::default()
            },
        );
    }

    /// A read saved pages before it stopped; the next pass goes on.
    pub fn progressed(&mut self, id: &SessionId) {
        if let Some(tracked) = self.0.get_mut(id) {
            tracked.failures = 0;
            tracked.retry_at = None;
        }
    }

    /// Records a failed read and returns when to try again.
    pub fn failed(&mut self, id: SessionId, now: Instant) -> Instant {
        let tracked = self.0.entry(id).or_default();
        tracked.failures = tracked.failures.saturating_add(1);
        let at = now + backoff(tracked.failures);
        tracked.retry_at = Some(at);
        at
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    RETRY_FIRST.saturating_mul(1 << doublings).min(RETRY_MOST)
}

/// Starts passes: the first at once, then on a change or when a session
/// comes due, never sooner than `PASS_GAP` after the previous one.
#[derive(Default)]
pub(super) struct Pacer {
    last: Option<Instant>,
}

impl Pacer {
    pub async fn next_pass(&mut self, changed: &Notify, due: Option<Instant>) {
        if let Some(last) = self.last {
            match due {
                Some(at) => tokio::select! {
                    () = sleep_until(at) => {}
                    () = changed.notified() => {}
                },
                None => changed.notified().await,
            }

            sleep_until(last + PASS_GAP).await;
        }

        self.last = Some(Instant::now());
    }
}

#[cfg(test)]
#[path = "../../tests/agents_search_schedule.rs"]
mod tests;
