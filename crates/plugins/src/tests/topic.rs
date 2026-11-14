use serde_json::json;

use super::*;

fn topic(name: &str, key: Option<&str>, machine: Option<&str>) -> Topic {
    Topic {
        name: name.to_owned(),
        key: key.map(str::to_owned),
        machine: machine.map(str::to_owned),
    }
}

fn delivery(topic: Topic, machine: Option<&str>, data: Value) -> Delivery {
    Delivery::new(&Publication {
        topic,
        machine: machine.map(str::to_owned),
        data,
    })
    .unwrap()
}

fn add(subscriptions: &mut Subscriptions, id: u32, name: &str) -> Result<(), TopicError> {
    subscriptions.add(id, &json!({ "name": name, "key": "s" }).to_string())
}

#[test]
fn ui_topics_need_no_permission() {
    let mut subscriptions = Subscriptions::new(&[]);

    assert_eq!(add(&mut subscriptions, 1, "ui.composer"), Ok(()));
}

#[test]
fn session_topics_need_the_permission() {
    let mut without = Subscriptions::new(&[Permission::UiAdd]);
    let mut with = Subscriptions::new(&[Permission::SessionsRead]);

    for name in ["agents", "agent", "session.turn"] {
        assert!(matches!(
            add(&mut without, 1, name),
            Err(TopicError::Denied {
                permission: Permission::SessionsRead,
                ..
            })
        ));
        assert_eq!(add(&mut with, 1, name), Ok(()));
    }
}

#[test]
fn other_names_are_refused() {
    let mut subscriptions = Subscriptions::new(&[Permission::SessionsRead, Permission::UiAdd]);

    for name in ["machines", "files", "ui", "uix.composer"] {
        assert_eq!(
            add(&mut subscriptions, 1, name),
            Err(TopicError::Unknown(name.to_owned())),
            "{name}"
        );
    }
}

#[test]
fn malformed_topics_are_invalid() {
    let mut subscriptions = Subscriptions::new(&[Permission::SessionsRead]);
    let long = "k".repeat(MAX_KEY_LEN + 1);

    for text in [
        "not json".to_owned(),
        json!({ "name": "agent", "extra": 1 }).to_string(),
        json!({ "name": 4 }).to_string(),
        json!({ "name": "a b" }).to_string(),
        json!({ "name": "*" }).to_string(),
        json!({ "name": "agent", "key": "" }).to_string(),
        json!({ "name": "agent", "key": long }).to_string(),
        json!({ "name": "agent", "machine": "m/1" }).to_string(),
    ] {
        assert!(
            matches!(subscriptions.add(1, &text), Err(TopicError::Invalid(_))),
            "{text}"
        );
    }
}

#[test]
fn at_most_sixteen_subscriptions() {
    let mut subscriptions = Subscriptions::new(&[]);

    for id in 0..16 {
        assert_eq!(add(&mut subscriptions, id, "ui.composer"), Ok(()));
    }
    assert_eq!(
        add(&mut subscriptions, 16, "ui.composer"),
        Err(TopicError::TooMany)
    );

    subscriptions.remove(3);
    assert_eq!(add(&mut subscriptions, 17, "ui.composer"), Ok(()));
}

#[test]
fn changes_are_taken_once_and_deduplicated() {
    let mut subscriptions = Subscriptions::new(&[]);

    assert_eq!(subscriptions.take_changed(), Some(Vec::new()));
    assert_eq!(subscriptions.take_changed(), None);

    add(&mut subscriptions, 1, "ui.b").unwrap();
    add(&mut subscriptions, 2, "ui.b").unwrap();
    add(&mut subscriptions, 3, "ui.a").unwrap();
    let topics = subscriptions.take_changed().unwrap();
    assert_eq!(
        topics.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["ui.a", "ui.b"]
    );

    subscriptions.remove(99);
    assert_eq!(subscriptions.take_changed(), None);
}

#[test]
fn a_large_event_is_refused() {
    let big = Publication {
        topic: topic("ui.composer", None, None),
        machine: None,
        data: json!({ "text": "x".repeat(MAX_EVENT_BYTES) }),
    };

    assert!(matches!(
        Delivery::new(&big),
        Err(TopicError::TooLarge(bytes)) if bytes > MAX_EVENT_BYTES
    ));
}

#[test]
fn an_event_is_serialized_with_its_topic() {
    let update = delivery(
        topic("agent", Some("s1"), Some("m1")),
        Some("m1"),
        json!({ "x": 1 }),
    );
    let value: Value = serde_json::from_str(update.json()).unwrap();

    assert_eq!(
        value,
        json!({
            "topic": { "name": "agent", "key": "s1", "machine": "m1" },
            "machine": "m1",
            "data": { "x": 1 }
        })
    );
}

#[test]
fn the_inbox_keeps_only_the_newest_of_each_stream() {
    let inbox = Inbox::default();
    let agent = topic("agent", Some("s1"), None);

    inbox.push(delivery(agent.clone(), Some("m1"), json!(1)));
    inbox.push(delivery(
        topic("ui.composer", None, None),
        None,
        json!("ui"),
    ));
    inbox.push(delivery(agent.clone(), Some("m2"), json!(2)));
    inbox.push(delivery(agent.clone(), Some("m1"), json!(3)));

    let taken: Vec<Value> = inbox
        .take()
        .iter()
        .map(|d| serde_json::from_str::<Value>(d.json()).unwrap()["data"].clone())
        .collect();
    assert_eq!(taken, [json!(3), json!("ui"), json!(2)]);
    assert!(inbox.take().is_empty());
}

#[test]
fn the_inbox_is_bounded() {
    let inbox = Inbox::default();

    for n in 0..MAX_PENDING + 10 {
        inbox.push(delivery(
            topic("agents", None, None),
            Some(&format!("m{n}")),
            json!(n),
        ));
    }

    assert_eq!(inbox.take().len(), MAX_PENDING);
}
