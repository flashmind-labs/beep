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
