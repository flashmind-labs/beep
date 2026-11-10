use super::*;

fn reader(id: Uuid, session: &str) -> (Reader, watch::Receiver<bool>) {
    let (stop, cancelled) = watch::channel(false);
    let (shown, _) = watch::channel(false);
    let followed = Followed {
        machine_id: "build-box".into(),
        agent: "codex".into(),
        session: session.into(),
    };
    (
        Reader {
            id,
            _stop: stop,
            shown,
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
    assert!(*b_shown.borrow(), "shown before it registered");
    let shown = readers.show("window", Some(first));
    assert_eq!(shown.map(|followed| followed.session), Some("one".into()));
    assert!(*a_shown.borrow());
    assert!(!*b_shown.borrow());
    assert!(readers.show("window", None).is_none());
    assert!(!*a_shown.borrow());
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
