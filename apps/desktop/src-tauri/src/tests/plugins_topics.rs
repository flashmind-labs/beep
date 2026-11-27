use std::sync::Mutex;

use super::*;

fn wanted(name: &str, key: Option<&str>, machine: Option<&str>) -> Topic {
    Topic {
        name: name.to_owned(),
        key: key.map(str::to_owned),
        machine: machine.map(str::to_owned),
    }
}

fn update(machine: &str, topic: NodeTopic) -> Update {
    Update {
        machine_id: machine.to_owned(),
        revision: 1,
        topic,
        query: None,
        initial: false,
        data: Some(serde_json::json!({ "rows": 2 })),
        seq: 0,
    }
}

#[test]
fn only_mesh_topics_are_followed() {
    let topics = [
        wanted("agents", None, None),
        wanted("agent", Some("one"), Some("m1")),
        wanted("ui.composer", None, None),
        wanted("session.turn", None, None),
        wanted("agents", None, None),
    ];

    let followed = interests(&topics);

    assert_eq!(followed.len(), 2);
    assert!(
        followed
            .iter()
            .all(|interest| ["agents", "agent"].contains(&interest.topic.name()))
    );
}

#[test]
fn an_update_answers_each_wanted_topic_it_matches() {
    let topics = [
        wanted("agents", None, None),
        wanted("agents", None, Some("m2")),
        wanted("agent", Some("one"), None),
        wanted("ui.composer", None, None),
    ];

    let found = publications(&topics, &update("m1", NodeTopic::new("agents")));

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].topic, topics[0]);
    assert_eq!(found[0].machine.as_deref(), Some("m1"));
    assert_eq!(found[0].data, serde_json::json!({ "rows": 2 }));
    assert!(
        publications(&topics, &update("m2", NodeTopic::new("agents"))).len() == 2,
        "a topic on one machine hears that machine"
    );
    assert_eq!(
        publications(&topics, &update("m1", NodeTopic::keyed("agent", "two"))).len(),
        0
    );
}

#[test]
fn a_followed_topic_reaches_the_host_without_the_page() {
    let events = WindowOutbox::default();
    let topics = vec![wanted("agents", None, None)];
    let (_keep, wanted_rx) = watch::channel(topics.clone());
    let heard = Arc::new(Mutex::new(Vec::new()));
    let sent = heard.clone();
    let mut reader = Reader::new(sink(wanted_rx, move |publication| {
        sent.lock().unwrap().push(publication);
    }));

    assert!(reader.follow(&events, interests(&topics)));
    events.dispatch(update("m1", NodeTopic::new("agents")));

    let heard = heard.lock().unwrap();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].topic, topics[0]);
    assert_eq!(heard[0].machine.as_deref(), Some("m1"));
    drop(heard);

    reader.follow(&events, Vec::new());
    assert!(!events.has_window(LABEL));
}
