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
