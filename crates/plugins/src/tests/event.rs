use super::*;

fn plugin() -> PluginId {
    "notes-one".parse().unwrap()
}

fn event(id: u64, action: Action) -> UiEvent {
    UiEvent {
        plugin: plugin(),
        id: CallbackId::from(id),
        action,
    }
}

fn text(id: u64, value: &str) -> UiEvent {
    event(
        id,
        Action::Text {
            value: value.to_owned(),
        },
    )
}

#[test]
fn actions_read_the_page_shape_and_become_arguments() {
    let press: Action = serde_json::from_str(r#"{"kind":"press"}"#).unwrap();
    let toggle: Action = serde_json::from_str(r#"{"kind":"toggle","value":true}"#).unwrap();
    let typed: Action = serde_json::from_str(r#"{"kind":"text","value":"a \"b\""}"#).unwrap();

    assert_eq!(press.argument(), "null");
    assert_eq!(toggle.argument(), "true");
    assert_eq!(typed.argument(), r#""a \"b\"""#);
    assert!(serde_json::from_str::<Action>(r#"{"kind":"drag"}"#).is_err());
}

#[test]
fn text_is_limited_to_four_kilobytes() {
    let fits = Action::Text {
        value: "x".repeat(MAX_TEXT_BYTES),
    };
    let over = Action::Text {
        value: "x".repeat(MAX_TEXT_BYTES + 1),
    };

    assert_eq!(fits.check(), Ok(()));
    assert_eq!(over.check(), Err(EventError::TooLong(MAX_TEXT_BYTES + 1)));
}

#[test]
fn thirty_events_a_second_pass() {
    let mut limiter = Limiter::default();
    let start = Instant::now();

    for i in 0..MAX_EVENTS_PER_SECOND {
        assert!(
            limiter.allow(start + Duration::from_millis(i as u64)),
            "{i}"
        );
    }
    assert!(!limiter.allow(start + Duration::from_millis(500)));
    assert!(!limiter.allow(start + Duration::from_millis(999)));

    // A second after the first events, room opens again.
    assert!(limiter.allow(start + Duration::from_millis(1000)));
}

#[test]
fn a_text_change_replaces_the_one_before_it() {
    let mailbox = Mailbox::default();

    mailbox.push(text(1, "a"));
    mailbox.push(text(1, "ab"));
    mailbox.push(text(2, "x"));
    mailbox.push(event(3, Action::Press));
    mailbox.push(event(3, Action::Press));

    let events = mailbox.take().events;
    assert_eq!(
        events,
        [
            text(1, "ab"),
            text(2, "x"),
            event(3, Action::Press),
            event(3, Action::Press)
        ]
    );
    assert!(mailbox.take().events.is_empty());
}

#[test]
fn a_full_mailbox_drops_new_events() {
    let mailbox = Mailbox::default();

    for _ in 0..MAX_PENDING + 10 {
        mailbox.push(event(1, Action::Press));
    }

    assert_eq!(mailbox.take().events.len(), MAX_PENDING);
}

#[test]
fn the_latest_instances_wait_for_the_thread() {
    let mailbox = Mailbox::default();
    assert!(mailbox.take().instances.is_none());

    mailbox.set_instances(Arc::new(vec![]));

    assert!(mailbox.take().instances.is_some());
    assert!(mailbox.take().instances.is_none());
}
