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
    let events = WindowOutbox::new(iroh::SecretKey::generate().public());
    let topics = vec![wanted("agents", None, None)];
    let heard = Arc::new(Heard::default());
    let mut reader = Reader::new(heard.clone().sink());
    let received = Mutex::new(Vec::new());
    let take = || {
        heard.take(&topics, |publication| {
            received.lock().unwrap().push(publication);
        });
    };

    assert!(reader.follow(&events, interests(&topics)));
    events.dispatch(update("m1", NodeTopic::new("agents")));
    take();
    assert_eq!(received.lock().unwrap().len(), 1);
    assert_eq!(received.lock().unwrap()[0].topic, topics[0]);
    assert_eq!(received.lock().unwrap()[0].machine.as_deref(), Some("m1"));

    // Nothing wanted ends the reader, and wanting a topic again starts it anew.
    reader.follow(&events, Vec::new());
    assert!(!events.has_window(LABEL));
    assert!(reader.follow(&events, interests(&topics)));
    assert!(events.has_window(LABEL));
    events.dispatch(update("m1", NodeTopic::new("agents")));
    take();
    assert_eq!(received.lock().unwrap().len(), 2);
}

#[test]
fn a_burst_reaches_the_host_as_its_newest_update() {
    let events = WindowOutbox::new(iroh::SecretKey::generate().public());
    let topics = vec![wanted("agents", None, None)];
    let heard = Arc::new(Heard::default());
    let mut reader = Reader::new(heard.clone().sink());
    assert!(reader.follow(&events, interests(&topics)));

    for rows in 1..=3 {
        let mut burst = update("m1", NodeTopic::new("agents"));
        burst.data = Some(serde_json::json!({ "rows": rows }));
        events.dispatch(burst);
    }
    let received = Mutex::new(Vec::new());
    heard.take(&topics, |publication| {
        received.lock().unwrap().push(publication)
    });

    let received = received.lock().unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].data, serde_json::json!({ "rows": 3 }));
}

#[test]
fn a_query_first_answer_does_not_reach_the_host() {
    let events = WindowOutbox::new(iroh::SecretKey::generate().public());
    let topics = vec![wanted("agents", None, None)];
    let heard = Arc::new(Heard::default());
    let mut reader = Reader::new(heard.clone().sink());
    assert!(reader.follow(&events, interests(&topics)));

    let mut answer = update("m1", NodeTopic::new("agents"));
    answer.query = Some(7);
    answer.initial = true;
    events.dispatch(answer);
    let received = Mutex::new(Vec::new());
    heard.take(&topics, |publication| {
        received.lock().unwrap().push(publication)
    });

    assert!(received.lock().unwrap().is_empty());
}
