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
async fn only_a_frame_postpones_the_silence_deadline() {
    let mut silence = Silence::new();

    // Nothing but time passes (window events do not reach it): it expires.
    tokio::time::advance(SILENT_AFTER - Duration::from_secs(1)).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(500), silence.expired())
            .await
            .is_err()
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    silence.expired().await;

    // A frame restarts the full wait.
    let mut silence = Silence::new();
    tokio::time::advance(SILENT_AFTER - Duration::from_secs(1)).await;
    silence.heard();
    tokio::time::advance(SILENT_AFTER - Duration::from_secs(1)).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(500), silence.expired())
            .await
            .is_err()
    );
}
