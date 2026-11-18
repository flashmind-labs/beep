use super::*;
use beep_proto::node::agent::SearchSession;

const LATER: Duration = Duration::from_secs(1);

fn conversation() -> Conversation {
    Conversation {
        id: "test-session".into(),
        last_activity: 10,
        metadata: SearchSession {
            agent: "codex".into(),
            cwd: "/path/to/work".into(),
            title: None,
            started_at: 1,
            managed: None,
        },
    }
}

#[tokio::test(start_paused = true)]
async fn a_session_is_read_immediately_and_again_when_it_changes() {
    let mut schedule = Schedule::default();
    let now = Instant::now();
    let mut one = conversation();

    assert_eq!(schedule.due(&one, now), Due::Now);

    schedule.indexed(conversation());
    assert_eq!(schedule.due(&one, now), Due::Current);
    one.metadata.title = Some("New title".into());
    assert_eq!(schedule.due(&one, now), Due::Now);
    one = conversation();
    one.last_activity += 1;
    assert_eq!(schedule.due(&one, now), Due::Now);
}

#[tokio::test(start_paused = true)]
async fn failures_back_off_until_a_read_saves_progress() {
    let mut schedule = Schedule::default();
    let session = conversation();
    let id = &session.id;
    let mut waits = Vec::new();

    for _ in 0..7 {
        let now = Instant::now();
        let at = schedule.failed(id.clone(), now);
        assert_eq!(schedule.due(&session, now), Due::At(at));
        waits.push((at - now).as_secs());
        tokio::time::advance(at - now).await;
        assert_eq!(schedule.due(&session, Instant::now()), Due::Now);
    }

    assert_eq!(waits, [30, 60, 120, 240, 480, 600, 600]);

    let now = Instant::now();
    schedule.failed(id.clone(), now);
    schedule.progressed(id);
    assert_eq!(schedule.due(&session, now), Due::Now);
    assert_eq!((schedule.failed(id.clone(), now) - now).as_secs(), 30);
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
