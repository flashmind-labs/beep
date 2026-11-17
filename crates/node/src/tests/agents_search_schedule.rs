use super::*;
use std::time::UNIX_EPOCH;

const LATER: Duration = Duration::from_secs(1);

#[tokio::test(start_paused = true)]
async fn a_session_is_read_once_quiet_and_not_again_until_it_changes() {
    let mut schedule = Schedule::default();
    let now = Instant::now();
    let activity = UNIX_EPOCH + Duration::from_secs(10);

    assert_eq!(
        schedule.due("build-box one", activity, Duration::from_secs(20), now),
        Due::At(now + Duration::from_secs(40)),
        "still running"
    );
    assert_eq!(
        schedule.due("build-box one", activity, QUIET, now),
        Due::Now
    );

    schedule.indexed("build-box one".into(), activity);
    assert_eq!(
        schedule.due("build-box one", activity, QUIET, now),
        Due::Current
    );
    assert_eq!(
        schedule.due("build-box one", activity + LATER, QUIET, now),
        Due::Now
    );

    schedule.clear();
    assert_eq!(
        schedule.due("build-box one", activity, QUIET, now),
        Due::Now
    );
}

#[tokio::test(start_paused = true)]
async fn failures_back_off_until_a_read_saves_progress() {
    let mut schedule = Schedule::default();
    let activity = UNIX_EPOCH;
    let id = "build-box one";
    let mut waits = Vec::new();

    for _ in 0..7 {
        let now = Instant::now();
        let at = schedule.failed(id.into(), now);
        assert_eq!(schedule.due(id, activity, QUIET, now), Due::At(at));
        waits.push((at - now).as_secs());
        tokio::time::advance(at - now).await;
        assert_eq!(schedule.due(id, activity, QUIET, Instant::now()), Due::Now);
    }

    assert_eq!(waits, [30, 60, 120, 240, 480, 600, 600]);

    let now = Instant::now();
    schedule.failed(id.into(), now);
    schedule.progressed(id);
    assert_eq!(schedule.due(id, activity, QUIET, now), Due::Now);
    assert_eq!((schedule.failed(id.into(), now) - now).as_secs(), 30);
}

#[tokio::test(start_paused = true)]
async fn changes_start_at_most_one_pass_a_minute() {
    let changed = Notify::new();
    let mut pacer = Pacer::default();
    let start = Instant::now();

    pacer.next_pass(&changed, None).await;
    assert_eq!(Instant::now(), start, "the first pass starts at once");

    for _ in 0..20 {
        changed.notify_one();
    }

    pacer.next_pass(&changed, None).await;
    assert_eq!(Instant::now(), start + PASS_GAP);

    // A session due sooner than the gap still waits for it.
    pacer
        .next_pass(&changed, Some(Instant::now() + LATER))
        .await;
    assert_eq!(Instant::now(), start + PASS_GAP * 2);

    // With no change, a pass starts when a session comes due.
    let due = Instant::now() + PASS_GAP * 3;
    pacer.next_pass(&changed, Some(due)).await;
    assert_eq!(Instant::now(), due);
}
