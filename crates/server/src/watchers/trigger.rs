//! When a chat is ready for a watcher: it has messages the watcher has not
//! read, no run is going on in it, and either it has been quiet for a while
//! or a task in it just ended. A watcher that failed waits longer after
//! each failure before it tries the same messages again.

use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;

/// A watcher that failed this many times in a row gives up on what it was
/// reading and moves past it.
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
mod tests {
    use super::*;

    const TIMING: Timing = Timing {
        quiet: TimeDelta::minutes(30),
        retry: TimeDelta::minutes(10),
    };

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::UNIX_EPOCH + TimeDelta::minutes(minutes)
    }

    fn chat(last_seq: i64, last_at: DateTime<Utc>) -> ChatActivity {
        ChatActivity {
            chat: Uuid::nil(),
            user: "alice".into(),
            last_seq,
            last_at,
            running: false,
        }
    }

    #[test]
    fn a_chat_is_ready_once_it_has_been_quiet_for_half_an_hour() {
        let talked = chat(10, at(100));
        assert!(!due(&talked, None, Cause::Sweep, at(100), TIMING));
        assert!(!due(&talked, None, Cause::Sweep, at(129), TIMING));
        assert!(due(&talked, None, Cause::Sweep, at(130), TIMING));
        // A new message starts the wait again.
        let again = chat(11, at(125));
        assert!(!due(&again, None, Cause::Sweep, at(130), TIMING));
        assert!(due(&again, None, Cause::Sweep, at(155), TIMING));
    }

    #[test]
    fn a_task_that_ended_makes_it_ready_now() {
        let ended = chat(10, at(100));
        assert!(due(&ended, None, Cause::TaskEnded, at(100), TIMING));
        let busy = ChatActivity {
            running: true,
            ..ended.clone()
        };
        assert!(!due(&busy, None, Cause::TaskEnded, at(100), TIMING));
        assert!(!due(&busy, None, Cause::Sweep, at(500), TIMING));
    }

    #[test]
    fn only_messages_past_its_place_count() {
        let quiet = chat(10, at(0));
        let read = WatcherState {
            reviewed_seq: 10,
            ..WatcherState::default()
        };
        assert!(!due(&quiet, Some(&read), Cause::Sweep, at(60), TIMING));
        assert!(!due(&quiet, Some(&read), Cause::TaskEnded, at(60), TIMING));
        let behind = WatcherState {
            reviewed_seq: 9,
            ..WatcherState::default()
        };
        assert!(due(&quiet, Some(&behind), Cause::Sweep, at(60), TIMING));
        assert!(!due(&chat(0, at(0)), None, Cause::Sweep, at(60), TIMING));
    }

    #[test]
    fn each_failure_waits_twice_as_long() {
        let quiet = chat(10, at(0));
        let failed = |failures| WatcherState {
            reviewed_seq: 0,
            failures,
            failed_at: Some(at(60)),
        };
        assert!(!due(&quiet, Some(&failed(1)), Cause::Sweep, at(69), TIMING));
        assert!(due(&quiet, Some(&failed(1)), Cause::Sweep, at(70), TIMING));
        assert!(!due(&quiet, Some(&failed(2)), Cause::Sweep, at(79), TIMING));
        assert!(due(&quiet, Some(&failed(2)), Cause::Sweep, at(80), TIMING));
        // A task ending does not skip the wait.
        assert!(!due(
            &quiet,
            Some(&failed(3)),
            Cause::TaskEnded,
            at(99),
            TIMING
        ));
        assert!(due(
            &quiet,
            Some(&failed(3)),
            Cause::TaskEnded,
            at(100),
            TIMING
        ));
        assert_eq!(TIMING.backoff(100), TimeDelta::minutes(10 * 1024));
    }
}
