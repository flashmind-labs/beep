use super::*;

#[test]
fn a_replaced_reader_and_its_late_detach_cannot_cancel_the_current_reader() {
    let follows = Follows::default();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let (old, old_cancelled) = watch::channel(false);
    follows
        .0
        .lock()
        .unwrap()
        .active
        .insert("window".into(), (first, old));
    let reading = Reading {
        follows: &follows,
        window: "window".into(),
        id: first,
    };
    let (new, new_cancelled) = watch::channel(false);
    follows
        .0
        .lock()
        .unwrap()
        .active
        .insert("window".into(), (second, new));
    assert!(old_cancelled.has_changed().is_err());
    drop(reading);
    follows.remove("window", first);
    assert!(new_cancelled.has_changed().is_ok());
    follows.clear("window");
    assert!(new_cancelled.has_changed().is_err());
}

#[test]
fn detaching_before_registration_does_not_leave_an_orphan_reader() {
    let mut readers = Readers::default();
    let id = Uuid::new_v4();
    readers.cancel("window", id);
    let (sender, receiver) = watch::channel(false);
    assert!(!readers.register("window", id, sender));
    assert!(readers.active.is_empty());
    assert!(receiver.has_changed().is_err());
}

#[test]
fn detaching_one_window_keeps_the_other_window_subscribed() {
    let follows = Follows::default();
    let id = Uuid::new_v4();
    let (a, a_cancelled) = watch::channel(false);
    let (b, b_cancelled) = watch::channel(false);
    follows
        .0
        .lock()
        .unwrap()
        .active
        .extend([("a".into(), (id, a)), ("b".into(), (id, b))]);
    follows.remove("a", id);
    assert!(a_cancelled.has_changed().is_err());
    assert!(b_cancelled.has_changed().is_ok());
}
