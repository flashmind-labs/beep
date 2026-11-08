//! When a chat is ready for a watcher: unread messages, no run, and quiet or a task just ended.

use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;

/// Failures in a row after which a watcher moves past what it was reading.
pub(crate) const MAX_FAILURES: i32 = 3;

/// Where a session is: its last message and when it last changed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChatActivity {
    pub chat: Uuid,
    pub user: String,
    /// The id of its newest message, 0 when it has none.
    pub last_seq: i64,
    pub last_at: DateTime<Utc>,
    /// A run is going on in it.
    pub running: bool,
}

/// How far one watcher read one chat.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WatcherState {
    /// The last message it read.
    pub reviewed_seq: i64,
    /// Failures in a row since then.
    pub failures: i32,
    pub failed_at: Option<DateTime<Utc>>,
}

/// Why a chat is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// The periodic sweep: only quiet chats are ready.
    Sweep,
    /// A task in the chat ended: ready now.
    TaskEnded,
}

/// The waits that decide readiness.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// How long a chat stays quiet before it is read.
    pub quiet: TimeDelta,
    /// The wait after a first failure, doubling with each one after.
    pub retry: TimeDelta,
}

impl Timing {
    /// How long a watcher waits after `failures` failures in a row.
    pub fn backoff(&self, failures: i32) -> TimeDelta {
        let doublings = u32::try_from(failures.saturating_sub(1).clamp(0, 10)).unwrap_or(0);
        self.retry * 2_i32.pow(doublings)
    }
}

/// Whether a watcher whose place is `state` should read `chat` at `now`.
pub(crate) fn due(
    chat: &ChatActivity,
    state: Option<&WatcherState>,
    cause: Cause,
    now: DateTime<Utc>,
    timing: Timing,
) -> bool {
    if chat.running {
        return false;
    }
    let state = state.cloned().unwrap_or_default();
    if chat.last_seq <= state.reviewed_seq {
        return false;
    }
    if state.failures > 0
        && let Some(failed_at) = state.failed_at
        && now < failed_at + timing.backoff(state.failures)
    {
        return false;
    }
    match cause {
        Cause::TaskEnded => true,
        Cause::Sweep => now - chat.last_at >= timing.quiet,
    }
}

#[cfg(test)]
#[path = "../../tests/memory_watchers_trigger.rs"]
mod tests;
