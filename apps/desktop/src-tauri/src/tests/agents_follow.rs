use super::*;
use beep_proto::watched;

fn reader(id: Uuid, session: &str) -> (Reader, watch::Receiver<bool>) {
    reader_on(id, session, "build-box", false)
}

fn reader_on(
    id: Uuid,
    session: &str,
    machine_id: &str,
    asking: bool,
) -> (Reader, watch::Receiver<bool>) {
    let (stop, cancelled) = watch::channel(false);
    let (shown, _) = watch::channel(false);
    let followed = SessionKey {
        machine_id: machine_id.into(),
        agent: "codex".into(),
        session: session.into(),
    };
    (
        Reader {
            id,
            _stop: stop,
            shown,
            asking: Arc::new(AtomicBool::new(asking)),
            followed,
        },
        cancelled,
    )
}

#[test]
fn one_window_follows_several_sessions_and_detaches_each_alone() {
    let mut readers = Readers::default();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let (a, a_cancelled) = reader(first, "one");
    let (b, b_cancelled) = reader(second, "two");
    assert!(readers.register("window", a));
    assert!(readers.register("window", b));
    readers.cancel("window", first);
    assert!(a_cancelled.has_changed().is_err());
    assert!(b_cancelled.has_changed().is_ok());
    readers.cancel("window", first);
    assert!(b_cancelled.has_changed().is_ok());
}

#[test]
fn only_the_reader_shown_counts_as_read() {
    let mut readers = Readers::default();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let (a, _) = reader(first, "one");
    let (b, _) = reader(second, "two");
    let (a_shown, b_shown) = (a.shown.subscribe(), b.shown.subscribe());
    readers.register("window", a);
    let shown = readers.show("window", Some(second));
    assert!(shown.is_none(), "not registered yet");
    readers.register("window", b);
    assert!(
        watched::read(&b_shown, |shown| *shown),
        "shown before it registered"
    );
    let shown = readers.show("window", Some(first));
    assert_eq!(shown.map(|followed| followed.session), Some("one".into()));
    assert!(watched::read(&a_shown, |shown| *shown));
    assert!(!watched::read(&b_shown, |shown| *shown));
    assert!(readers.show("window", None).is_none());
    assert!(!watched::read(&a_shown, |shown| *shown));
}

#[test]
fn a_window_past_its_reader_limit_cancels_the_oldest() {
    let mut readers = Readers::default();
    let mut cancelled = Vec::new();

    for at in 0..=WINDOW_READERS {
        let (next, receiver) = reader(Uuid::new_v4(), &at.to_string());
        readers.register("window", next);
        cancelled.push(receiver);
    }

    assert!(cancelled[0].has_changed().is_err());
    assert!(
        cancelled[1..]
            .iter()
            .all(|receiver| receiver.has_changed().is_ok())
    );
}

#[test]
fn detaching_before_registration_does_not_leave_an_orphan_reader() {
    let mut readers = Readers::default();
    let id = Uuid::new_v4();
    readers.cancel("window", id);
    let (next, receiver) = reader(id, "one");
    assert!(!readers.register("window", next));
    assert!(readers.active.is_empty());
    assert!(receiver.has_changed().is_err());
}

#[test]
fn a_finished_reader_leaves_and_closing_the_window_cancels_the_rest() {
    let follows = Follows::default();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let (a, a_cancelled) = reader(first, "one");
    let (b, b_cancelled) = reader(second, "two");
    follows.readers().register("a", a);
    follows.readers().register("b", b);
    drop(Reading {
        follows: &follows,
        window: "a".into(),
        id: first,
    });
    assert!(a_cancelled.has_changed().is_err());
    assert!(b_cancelled.has_changed().is_ok());
    follows.readers().show("b", Some(second));
    follows.clear("b");
    assert!(b_cancelled.has_changed().is_err());
    assert!(follows.readers().shown.is_empty());
}

#[test]
fn a_window_past_its_reader_limit_keeps_the_shown_and_asking_readers() {
    let mut readers = Readers::default();
    let shown = Uuid::new_v4();
    let (first, first_cancelled) = reader(shown, "shown");
    readers.register("window", first);
    readers.show("window", Some(shown));
    let (second, asking_cancelled) = reader_on(Uuid::new_v4(), "asks", "build-box", true);
    readers.register("window", second);
    let mut rest = Vec::new();

    for at in 2..=WINDOW_READERS {
        let (next, receiver) = reader(Uuid::new_v4(), &at.to_string());
        readers.register("window", next);
        rest.push(receiver);
    }

    assert!(first_cancelled.has_changed().is_ok());
    assert!(asking_cancelled.has_changed().is_ok());
    assert!(rest[0].has_changed().is_err());
    assert!(
        rest[1..]
            .iter()
            .all(|receiver| receiver.has_changed().is_ok())
    );
}

#[test]
fn a_full_node_releases_the_oldest_quiet_background_reader_on_it() {
    let mut readers = Readers::default();
    let shown = Uuid::new_v4();
    let (elsewhere, elsewhere_cancelled) = reader_on(Uuid::new_v4(), "a", "gpu-box", false);
    let (asks, asks_cancelled) = reader_on(Uuid::new_v4(), "b", "build-box", true);
    let (quiet, quiet_cancelled) = reader_on(Uuid::new_v4(), "c", "build-box", false);
    let (newer, newer_cancelled) = reader_on(Uuid::new_v4(), "d", "build-box", false);
    let (open, _) = reader(shown, "shown");

    for next in [elsewhere, asks, quiet, newer, open] {
        readers.register("window", next);
    }

    readers.show("window", Some(shown));
    assert!(readers.release_on("window", "build-box", shown));
    assert!(quiet_cancelled.has_changed().is_err());
    assert!(elsewhere_cancelled.has_changed().is_ok());
    assert!(asks_cancelled.has_changed().is_ok());
    assert!(newer_cancelled.has_changed().is_ok());

    assert!(readers.release_on("window", "build-box", shown));
    assert!(newer_cancelled.has_changed().is_err());
    assert!(!readers.release_on("window", "build-box", shown));
    assert!(asks_cancelled.has_changed().is_ok());
}

#[test]
fn a_full_node_is_told_apart_from_other_follow_errors() {
    assert!(node_full("too many session readers"));
    assert!(!node_full("session not found"));
}

#[tokio::test(start_paused = true)]
async fn only_a_frame_postpones_the_silence_deadline_in_the_follow_loop() {
    let (frames, mut incoming) = tokio::sync::mpsc::unbounded_channel::<u8>();
    let (_screen, mut on_screen) = watch::channel(false);
    let (window, mut window_shown) = watch::channel(None);
    let mut silence = Silence::new();
    let step = Duration::from_secs(10);

    // Window events every 10 s, three times 10 s, then the deadline passes.
    for shown in [Some(true), Some(false), Some(true)] {
        tokio::time::advance(step).await;
        window.send_replace(shown);
        let woke = wake::<u8, DesktopError>(
            async { Ok(incoming.recv().await) },
            &mut silence,
            None,
            None,
            &mut on_screen,
            &mut window_shown,
            true,
        )
        .await
        .unwrap();
        assert!(matches!(woke, Woke::Window { closed: false }));
    }

    tokio::time::advance(SILENT_AFTER - 3 * step).await;
    let silent = wake::<u8, DesktopError>(
        async { Ok(incoming.recv().await) },
        &mut silence,
        None,
        None,
        &mut on_screen,
        &mut window_shown,
        true,
    )
    .await;
    assert!(
        matches!(silent, Err(DesktopError::BadRequest(reason)) if reason.contains("stopped responding"))
    );

    // A frame restarts the full wait.
    let mut silence = Silence::new();
    tokio::time::advance(SILENT_AFTER - Duration::from_secs(1)).await;
    frames.send(7).unwrap();
    let woke = wake::<u8, DesktopError>(
        async { Ok(incoming.recv().await) },
        &mut silence,
        None,
        None,
        &mut on_screen,
        &mut window_shown,
        true,
    )
    .await
    .unwrap();
    assert!(matches!(woke, Woke::Frame(7)));
    tokio::time::advance(SILENT_AFTER - Duration::from_secs(1)).await;
    let waiting = tokio::time::timeout(
        Duration::from_millis(500),
        wake::<u8, DesktopError>(
            async { Ok(incoming.recv().await) },
            &mut silence,
            None,
            None,
            &mut on_screen,
            &mut window_shown,
            true,
        ),
    )
    .await;
    assert!(waiting.is_err());
}

#[test]
fn follow_state_reconstructs_snapshots_from_patches() {
    use beep_proto::node::agent::{Entry, EntryKind};

    let mut state = FollowState::default();
    let cursor = |start, next| SessionCursor {
        start,
        next,
        fingerprint: format!("{start}:{next}"),
        prefix_next: next.saturating_sub(1),
        prefix_fingerprint: String::new(),
    };
    let initial = SessionSnapshot {
        source: SessionSource::Owned(Box::default()),
        entries: vec![
            Entry::new(EntryKind::User, "hi".into()),
            Entry::new(EntryKind::Agent, "hel".into()),
        ],
        earlier: 0,
        cursor: cursor(0, 2),
        resumed: false,
        controls: true,
    };
    state.record_snapshot(&initial, true);

    let appended = state
        .apply_patch(SessionPatch {
            source: None,
            cursor: cursor(0, 2),
            ops: vec![EntryPatch::AppendText {
                index: 1,
                text: "lo".into(),
            }],
        })
        .unwrap();
    assert_eq!(appended.earlier, 1);
    assert_eq!(appended.entries.len(), 1);
    assert_eq!(appended.entries[0].text, "hello");

    let replaced = state
        .apply_patch(SessionPatch {
            source: None,
            cursor: cursor(0, 3),
            ops: vec![EntryPatch::Replace {
                index: 2,
                entry: Entry::new(EntryKind::Agent, "next".into()),
            }],
        })
        .unwrap();
    assert_eq!(replaced.earlier, 2);
    assert_eq!(replaced.entries.len(), 1);
    assert_eq!(replaced.entries[0].text, "next");

    let truncated = state
        .apply_patch(SessionPatch {
            source: None,
            cursor: cursor(0, 2),
            ops: vec![EntryPatch::Truncate { next: 2 }],
        })
        .unwrap();
    assert_eq!(truncated.earlier, 2);
    assert!(truncated.entries.is_empty());

    // Out-of-sync cursor lengths return an error instead of slicing out of bounds.
    assert!(
        state
            .apply_patch(SessionPatch {
                source: None,
                cursor: cursor(0, 5),
                ops: Vec::new(),
            })
            .is_err()
    );
    assert!(
        state
            .apply_patch(SessionPatch {
                source: None,
                cursor: cursor(0, 1),
                ops: Vec::new(),
            })
            .is_err()
    );
}
