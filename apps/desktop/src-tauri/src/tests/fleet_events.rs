use super::*;
use beep_proto::watched;

#[test]
fn dotted_topics_cross_ipc_and_prefixes_compact_without_widening() {
    let interest: Interest = serde_json::from_value(serde_json::json!({
        "machine": null,
        "topic": "files.org.repo"
    }))
    .unwrap();
    assert_eq!(interest.topic.resource_segments(), ["org", "repo"]);
    assert_eq!(
        serde_json::to_value(&interest).unwrap()["topic"],
        "files.org.repo"
    );
    let topics = [
        "files.org.*",
        "files.org.repo",
        "files.other.repo",
        "files.org%2Erepo",
    ]
    .map(|topic| Topic::parse(topic).unwrap());
    let compacted = compact_topics(topics.to_vec());
    assert_eq!(compacted.len(), 3);
    assert!(compacted.contains(&Topic::parse("files.org.*").unwrap()));
    assert!(compacted.contains(&Topic::parse("files.other.repo").unwrap()));
    assert!(compacted.contains(&Topic::parse("files.org%2Erepo").unwrap()));
}

fn interest(name: &str, machine: Option<EndpointId>) -> Interest {
    Interest {
        machine: machine.map(|id| id.to_string()),
        topic: Topic::new(name),
    }
}
fn register(
    events: &Events,
    label: &str,
    id: Uuid,
    topics: Vec<Interest>,
    send: Sink,
) -> watch::Receiver<()> {
    events
        .registry
        .lock()
        .unwrap()
        .register(label, id, topics, send)
        .unwrap()
        .unwrap()
}

#[test]
fn windows_share_demand_and_unused_nodes_are_not_dialed() {
    let events = Events::default();
    let local = iroh::SecretKey::generate().public();
    let remote = iroh::SecretKey::generate().public();
    let topics = vec![interest("files", Some(remote))];
    let first = Uuid::new_v4();
    let _one = register(&events, "one", first, topics.clone(), Arc::new(|_| true));
    let _two = register(
        &events,
        "two",
        Uuid::new_v4(),
        topics.clone(),
        Arc::new(|_| true),
    );
    events.changed();
    assert_eq!(watched::read_sent(&events.demand, Clone::clone), topics);
    assert_eq!(topics_for(&topics, remote, local), [Topic::new("files")]);
    assert!(topics_for(&topics, local, local).is_empty());
    events.remove("one", first);
    assert_eq!(watched::read_sent(&events.demand, Clone::clone), topics);
    events.clear("two");
    assert!(watched::read_sent(&events.demand, |demand| demand.is_empty()));
    assert!(topics_for(&[interest("telemetry", None)], remote, local).is_empty());
}

#[test]
fn cancelling_before_registration_prevents_a_late_reader() {
    let mut registry = Registry::default();
    let id = Uuid::new_v4();
    registry.remove("view", id);
    assert!(
        registry
            .register(
                "view",
                id,
                vec![interest("agents", None)],
                Arc::new(|_| true)
            )
            .unwrap()
            .is_none()
    );
    assert!(registry.interests().is_empty());
}

#[test]
fn early_topic_changes_and_late_cleanup_preserve_the_replacement() {
    let mut registry = Registry::default();
    let id = Uuid::new_v4();
    let topics = vec![interest("reviews", None)];
    registry.topics("view", id, topics.clone()).unwrap();
    let _reader = registry
        .register("view", id, vec![], Arc::new(|_| true))
        .unwrap()
        .unwrap();
    registry.remove("view", Uuid::new_v4());
    assert_eq!(registry.interests(), topics);
}

#[test]
fn dispatch_filters_topics_and_keeps_failed_windows_out_of_demand() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let _one = register(
        &events,
        "one",
        Uuid::new_v4(),
        vec![interest("reviews", Some(machine))],
        Arc::new(move |event| {
            output.lock().unwrap().push(event);
            true
        }),
    );
    let _two = register(
        &events,
        "two",
        Uuid::new_v4(),
        vec![interest("files", None)],
        Arc::new(|_| false),
    );
    let update = Update {
        machine_id: machine.to_string(),
        topic: Topic::new("reviews"),
        revision: 1,
        query: None,
        initial: false,
        data: None,
        seq: 0,
    };
    events.dispatch(update.clone());
    assert_eq!(heard.lock().unwrap().len(), 1);
    events.dispatch(Update {
        topic: Topic::new("files"),
        ..update
    });
    assert!(!events.registry.lock().unwrap().windows.contains_key("two"));
}

#[tokio::test]
async fn a_lost_command_reply_has_an_unknown_outcome_and_is_not_replayed() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let _reader = register(
        &events,
        "view",
        Uuid::new_v4(),
        vec![interest("agents", None)],
        Arc::new(|_| true),
    );
    let (sender, mut input) = mpsc::channel(1);
    events
        .commands
        .lock()
        .unwrap()
        .insert(machine, (Uuid::new_v4(), sender));
    let receive = async {
        let call = input.recv().await.unwrap();
        drop(call.reply);
    };
    let operation = Operation::Process(beep_proto::node::process::Request::List {});
    let (result, ()) = tokio::join!(events.call("view", machine, operation), receive);
    assert!(matches!(result, Err(DesktopError::Unsure(_))));
    assert!(input.try_recv().is_err());
}

#[test]
fn a_stale_connection_cannot_remove_its_replacement() {
    let machine = iroh::SecretKey::generate().public();
    let (sender, _) = mpsc::channel(1);
    let commands = Commands::default();
    commands
        .lock()
        .unwrap()
        .insert(machine, (Uuid::new_v4(), sender));
    drop(Registered {
        machine,
        commands: &commands,
        generation: Uuid::new_v4(),
    });
    assert!(commands.lock().unwrap().contains_key(&machine));
}

fn update(machine: EndpointId, sequence: u64, topic: &str) -> Update {
    Update {
        machine_id: machine.to_string(),
        revision: sequence,
        query: None,
        initial: false,
        topic: Topic::new(topic),
        data: None,
        seq: 0,
    }
}

#[test]
fn an_attaching_window_is_primed_when_shared_demand_does_not_change() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let topics = vec![interest("reviews", Some(machine))];
    let _first = events
        .register(
            "one",
            Uuid::new_v4(),
            topics.clone(),
            Arc::new(|_| true),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    events.dispatch(update(machine, 7, "reviews"));
    let demand = watched::read_sent(&events.demand, Clone::clone);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let _second = events
        .register(
            "two",
            Uuid::new_v4(),
            topics,
            Arc::new(move |event| {
                output.lock().unwrap().push(event);
                true
            }),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(watched::read_sent(&events.demand, Clone::clone), demand);
    events.dispatch(update(machine, 8, "reviews"));
    let heard = heard.lock().unwrap();
    assert_eq!(heard.len(), 2);
    assert_eq!(heard[0].revision, 7);
    assert_eq!(heard[0].topic, Topic::new("reviews"));
    assert_eq!(heard[1].revision, 8);
}

#[test]
fn a_destroyed_window_cannot_register_or_retain_demand() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let events = Events::default();
    let topics = vec![interest("agents", None)];
    assert!(
        events
            .register(
                "gone",
                Uuid::new_v4(),
                topics.clone(),
                Arc::new(|_| true),
                || false,
                None
            )
            .unwrap()
            .is_none()
    );
    let checks = AtomicUsize::new(0);
    assert!(
        events
            .register(
                "closing",
                Uuid::new_v4(),
                topics,
                Arc::new(|_| true),
                || checks.fetch_add(1, Ordering::SeqCst) == 0,
                None
            )
            .unwrap()
            .is_none()
    );
    assert!(events.registry.lock().unwrap().windows.is_empty());
    assert!(watched::read_sent(&events.demand, |demand| demand.is_empty()));
}

#[test]
fn an_overflowing_window_does_not_disrupt_existing_subscriptions() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let mut topics: Vec<_> = (0..events::MAX_TOPICS)
        .map(|number| interest(&format!("plugin_{number}"), None))
        .collect();
    topics.sort();
    let _first = events
        .register(
            "one",
            Uuid::new_v4(),
            topics.clone(),
            Arc::new(|_| true),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    let overflow = events.register(
        "two",
        Uuid::new_v4(),
        vec![interest("another", Some(machine))],
        Arc::new(|_| true),
        || true,
        None,
    );
    assert!(matches!(overflow, Err(DesktopError::BadRequest(_))));
    assert_eq!(watched::read_sent(&events.demand, Clone::clone), topics);
    assert!(!events.registry.lock().unwrap().windows.contains_key("two"));
}

#[test]
fn a_rejected_topic_change_keeps_the_previous_window_topics() {
    let mut registry = Registry::default();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let topics: Vec<_> = (0..events::MAX_TOPICS)
        .map(|number| interest(&format!("plugin_{number}"), None))
        .collect();
    let _first = registry
        .register("one", first, topics.clone(), Arc::new(|_| true))
        .unwrap()
        .unwrap();
    let _second = registry
        .register("two", second, vec![topics[0].clone()], Arc::new(|_| true))
        .unwrap()
        .unwrap();
    assert!(
        registry
            .topics("two", second, vec![interest("another", None)])
            .is_err()
    );
    assert_eq!(registry.windows["two"].topics, [topics[0].clone()]);
}

#[test]
fn topic_limits_apply_separately_to_distinct_nodes() {
    let mut registry = Registry::default();

    for window in ["one", "two"] {
        let machine = iroh::SecretKey::generate().public();
        let topics = (0..events::MAX_TOPICS)
            .map(|number| interest(&format!("plugin_{number}"), Some(machine)))
            .collect();
        let _reader = registry
            .register(window, Uuid::new_v4(), topics, Arc::new(|_| true))
            .unwrap()
            .unwrap();
    }

    assert_eq!(registry.windows.len(), 2);
}

#[test]
fn broad_namespace_subscriptions_compact_redundant_resource_topics() {
    let events = Events::default();
    let local = iroh::SecretKey::generate().public();
    let topics = vec![interest("reviews", None)];
    let _first = events
        .register(
            "one",
            Uuid::new_v4(),
            topics,
            Arc::new(|_| true),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    let resources: Vec<_> = (0..events::MAX_TOPICS)
        .map(|number| Interest {
            machine: None,
            topic: Topic::keyed("reviews", format!("review_{number}")),
        })
        .collect();
    let _second = events
        .register(
            "two",
            Uuid::new_v4(),
            resources,
            Arc::new(|_| true),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        watched::read_sent(&events.demand, |demand| topics_for(demand, local, local)),
        [Topic::new("reviews")]
    );
}

#[test]
fn controller_topics_are_unscoped_and_ignore_other_nodes_snapshots() {
    let events = Events::default();
    let local = iroh::SecretKey::generate().public();
    let remote = iroh::SecretKey::generate().public();
    events.controller.set(local).unwrap();
    assert!(validate(&[interest("telemetry", Some(remote))]).is_err());
    assert!(validate(&[interest("telemetry", None)]).is_ok());
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let _reader = events
        .register(
            "one",
            Uuid::new_v4(),
            vec![interest("telemetry", None)],
            Arc::new(move |event| {
                output.lock().unwrap().push(event);
                true
            }),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    events.dispatch(Update {
        ..update(remote, 1, "*")
    });
    assert!(heard.lock().unwrap().is_empty());
    events.dispatch(Update {
        ..update(local, 1, "*")
    });
    assert_eq!(heard.lock().unwrap().len(), 1);
}

#[test]
fn home_keeps_agent_sources_live_on_every_node() {
    let local = iroh::SecretKey::generate().public();
    let remote = iroh::SecretKey::generate().public();
    let interests = [interest("home", None)];

    for machine in [local, remote] {
        let topics = topics_for(&interests, machine, local);

        for source in home_sources() {
            assert!(topics.contains(&source));
        }

        assert_eq!(topics.contains(&Topic::new("home")), machine == local);
    }
}

#[test]
fn retained_snapshots_bound_each_node_and_all_nodes_and_remove_oversized_replacements() {
    let machine = iroh::SecretKey::generate().public();
    let mut latest = Latest::default();
    let first = Update {
        query: Some(1),
        ..update(machine, 1, "files")
    };
    let second = Update {
        query: Some(2),
        ..first.clone()
    };
    latest.insert(first.clone(), 10 * 1024 * 1024);
    latest.insert(second.clone(), 10 * 1024 * 1024);
    assert_eq!(latest.values().count(), 1);
    assert_eq!(latest.values().next().unwrap().query, Some(2));
    latest.insert(second, MAX_NODE_SNAPSHOT_BYTES + 1);
    assert_eq!(latest.values().count(), 0);

    for _ in 0..10 {
        latest.insert(
            update(iroh::SecretKey::generate().public(), 1, "files"),
            10 * 1024 * 1024,
        );
    }

    assert!(latest.bytes <= MAX_SNAPSHOT_BYTES);
    assert_eq!(latest.values().count(), 6);
}

#[test]
fn a_folder_update_kept_for_a_hidden_window_names_the_paths_of_those_it_replaced() {
    use crate::fleet::view_signal::changed_paths;

    let machine = iroh::SecretKey::generate().public();
    let mut latest = Latest::default();
    let listing = |paths: &[&str]| Update {
        data: Some(serde_json::json!({ "Sessions": { "Tree": { "tree": {}, "changed": paths } } })),
        ..update(machine, 1, "files")
    };

    latest.insert(listing(&["a.md"]), 10);
    latest.insert(listing(&["b.ts"]), 10);

    let kept = latest.values().next().unwrap();
    assert_eq!(changed_paths(kept), ["a.md", "b.ts"]);
}

#[test]
fn resource_directories_remove_absent_defaults_without_removing_queries_or_other_scopes() {
    let machine = iroh::SecretKey::generate().public();
    let peer = iroh::SecretKey::generate().public();
    let removed = Topic::parse("files.org.removed").unwrap();
    let present = Topic::parse("files.org.present").unwrap();
    let scope = Topic::parse("files.org.*").unwrap();
    let mut latest = Latest::default();
    for event in [
        Update {
            topic: removed.clone(),
            ..update(machine, 1, "files")
        },
        Update {
            topic: present.clone(),
            ..update(machine, 2, "files")
        },
        Update {
            topic: removed.clone(),
            query: Some(7),
            ..update(machine, 3, "files")
        },
        Update {
            topic: removed.clone(),
            ..update(peer, 1, "files")
        },
        Update {
            topic: scope.clone(),
            ..update(machine, 4, "files")
        },
        Update {
            topic: Topic::parse("files.other.repository").unwrap(),
            ..update(machine, 5, "files")
        },
    ] {
        latest.insert(event, 100);
    }
    latest.resources(&machine.to_string(), &scope, &[present]);
    assert_eq!(latest.values().count(), 5);
    assert_eq!(latest.bytes, 500);
    assert!(
        !latest
            .values()
            .any(|event| event.machine_id == machine.to_string()
                && event.topic == removed
                && event.query.is_none())
    );
    latest.resources(&machine.to_string(), &scope, &[]);
    assert_eq!(latest.values().count(), 4);
}

#[test]
fn a_new_node_stream_clears_old_resources_and_keeps_controller_owned_views() {
    let machine = iroh::SecretKey::generate().public();
    let peer = iroh::SecretKey::generate().public();
    let mut latest = Latest::default();
    latest.insert(update(machine, 1, "files"), 100);
    latest.insert(update(machine, 2, "home"), 100);
    latest.insert(update(machine, 3, "forge"), 100);
    latest.insert(update(peer, 1, "files"), 100);
    latest.clear_node(&machine.to_string(), true);
    assert_eq!(latest.values().count(), 3);
    assert!(
        !latest
            .values()
            .any(|event| event.machine_id == machine.to_string() && event.topic.name() == "files")
    );
    latest.clear_node(&machine.to_string(), false);
    assert_eq!(latest.values().count(), 1);
    assert_eq!(latest.values().next().unwrap().machine_id, peer.to_string());
}

#[test]
fn new_windows_request_fresh_snapshots_and_retired_query_states_are_not_primed() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let mut refresh = events.refresh.subscribe();
    let topics = vec![interest("processes", Some(machine))];
    let _one = events
        .register(
            "one",
            Uuid::new_v4(),
            topics.clone(),
            Arc::new(|_| true),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(refresh.has_changed().unwrap());
    watched::read_and_update(&mut refresh, |_| ());
    events.dispatch(Update {
        query: Some(1),
        ..update(machine, 1, "processes")
    });
    events.forget_queries(machine, &[]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let _two = events
        .register(
            "two",
            Uuid::new_v4(),
            topics,
            Arc::new(move |update| {
                output.lock().unwrap().push(update);
                true
            }),
            || true,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(heard.lock().unwrap().is_empty());
    assert!(refresh.has_changed().unwrap());
}

#[test]
fn output_subscribers_receive_their_projected_state_under_the_requested_topic() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let _reader = register(
        &events,
        "one",
        Uuid::new_v4(),
        vec![interest("process-output", Some(machine))],
        Arc::new(move |event| {
            output.lock().unwrap().push(event);
            true
        }),
    );
    let data = Some(serde_json::json!({ "lines": ["output"] }));
    events.dispatch_node(Update {
        query: Some(9),
        data: data.clone(),
        ..update(machine, 1, "processes")
    });
    let heard = heard.lock().unwrap();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].topic, Topic::new("process-output"));
    assert_eq!(heard[0].query, Some(9));
    assert_eq!(heard[0].data, data);
}

#[test]
fn scheduling_workers_does_not_consume_a_pending_mesh_swap() {
    let (source, changed) = watch::channel(Opened::Opening);
    source.send_replace(Opened::Failed("closed".into()));
    assert!(!mesh_is_open(&changed));
    assert!(changed.has_changed().unwrap());
}

#[test]
fn an_unchanged_view_is_not_sent_again_and_a_new_window_still_gets_it() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let _one = register(
        &events,
        "one",
        Uuid::new_v4(),
        vec![interest("sync", None)],
        Arc::new(move |event| {
            output.lock().unwrap().push(event);
            true
        }),
    );
    let view = serde_json::json!({ "views": [{ "command": "project_sync", "value": [] }] });

    events.publish_views(machine, Topic::new("sync"), view.clone());
    events.publish_views(machine, Topic::new("sync"), view.clone());
    assert_eq!(heard.lock().unwrap().len(), 1);

    let changed = serde_json::json!({ "views": [{ "command": "project_sync", "value": [1] }] });
    events.publish_views(machine, Topic::new("sync"), changed.clone());
    assert_eq!(heard.lock().unwrap().len(), 2);

    let primed = events
        .registry
        .lock()
        .unwrap()
        .latest
        .values()
        .filter(|update| update.topic == Topic::new("sync"))
        .map(|update| update.data.clone())
        .collect::<Vec<_>>();
    assert_eq!(primed, [Some(changed)]);
}

#[test]
fn a_window_that_takes_a_topic_is_primed_with_its_held_view() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let _one = register(
        &events,
        "one",
        Uuid::new_v4(),
        vec![interest("sync", None)],
        Arc::new(|_| true),
    );
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let two = Uuid::new_v4();
    let _two = register(
        &events,
        "two",
        two,
        vec![interest("usage", None)],
        Arc::new(move |event| {
            output.lock().unwrap().push(event);
            true
        }),
    );
    let view = serde_json::json!({ "views": [] });
    events.publish_views(machine, Topic::new("sync"), view.clone());
    assert!(heard.lock().unwrap().is_empty());

    events
        .retopic(
            "two",
            two,
            vec![interest("usage", None), interest("sync", None)],
        )
        .unwrap();
    events.publish_views(machine, Topic::new("sync"), view.clone());

    let heard = heard.lock().unwrap();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].data, Some(view));
}

fn heard_window(
    events: &Events,
    label: &str,
    id: Uuid,
    topics: Vec<Interest>,
) -> Arc<Mutex<Vec<Update>>> {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();
    let registered = events
        .register(
            label,
            id,
            topics,
            Arc::new(move |event| {
                output.lock().unwrap().push(event);
                true
            }),
            || true,
            None,
        )
        .unwrap();
    assert!(registered.is_some());
    heard
}

#[test]
fn a_window_shown_again_within_grace_gets_only_what_changed_without_a_refresh() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let topics = vec![
        interest("reviews", Some(machine)),
        interest("files", Some(machine)),
        interest("processes", Some(machine)),
    ];
    let id = Uuid::new_v4();
    let heard = heard_window(&events, "one", id, topics.clone());

    for (sequence, topic) in [(1, "reviews"), (2, "files"), (3, "processes")] {
        events.dispatch(update(machine, sequence, topic));
    }

    assert_eq!(heard.lock().unwrap().len(), 3);
    let demand = watched::read_sent(&events.demand, Clone::clone);
    let mut refresh = events.refresh.subscribe();
    watched::read_and_update(&mut refresh, |_| ());

    assert!(events.pause("one", id, None).unwrap());
    assert!(!events.pause("one", id, None).unwrap());
    events.dispatch(update(machine, 4, "reviews"));
    events.dispatch(update(machine, 5, "reviews"));
    assert_eq!(
        heard.lock().unwrap().len(),
        3,
        "a hidden window is sent nothing"
    );
    assert_eq!(
        watched::read_sent(&events.demand, Clone::clone),
        demand,
        "node streams stay open while hidden"
    );
    assert!(!events.controller_wanted(&Topic::new("home")));

    assert_eq!(
        events.resume("one", id, None).unwrap(),
        Some(Primed::Changes)
    );
    let heard = heard.lock().unwrap();
    assert_eq!(
        heard.len(),
        4,
        "showing sends one update, the newest changed one"
    );
    assert_eq!(heard[3].revision, 5);
    assert!(
        !refresh.has_changed().unwrap(),
        "showing asks no node to resend"
    );
    assert_eq!(watched::read_sent(&events.demand, Clone::clone), demand);

    // The old hide and show ended the reader and attached a new one, which was sent every held
    // update and made every node stream resend its state.
    let reattached = heard_window(&events, "two", Uuid::new_v4(), topics.clone());
    assert_eq!(reattached.lock().unwrap().len(), 3);
    assert!(
        !refresh.has_changed().unwrap(),
        "what was primed from the cache is not asked for again"
    );

    // A topic nothing was kept of is asked for.
    let mut more = topics;
    more.push(interest("usage", Some(machine)));
    heard_window(&events, "three", Uuid::new_v4(), more);
    assert!(refresh.has_changed().unwrap());
}

#[test]
fn a_window_hidden_past_grace_releases_its_streams_and_restarts_them_when_shown() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let id = Uuid::new_v4();
    let heard = heard_window(&events, "one", id, vec![interest("reviews", Some(machine))]);
    events.dispatch(update(machine, 1, "reviews"));
    events.registry.lock().unwrap().grace = Duration::ZERO;

    assert!(events.pause("one", id, None).unwrap());
    events.changed();
    assert!(watched::read_sent(&events.demand, |demand| demand.is_empty()));
    assert_eq!(events.registry.lock().unwrap().latest.values().count(), 0);
    events.dispatch(update(machine, 2, "reviews"));
    assert_eq!(events.registry.lock().unwrap().latest.values().count(), 0);

    assert_eq!(
        events.resume("one", id, None).unwrap(),
        Some(Primed::Expired)
    );
    assert_eq!(watched::read_sent(&events.demand, |demand| demand.len()), 1);
    events.dispatch(update(machine, 3, "reviews"));
    let heard = heard.lock().unwrap();
    assert_eq!(
        heard
            .iter()
            .map(|update| update.revision)
            .collect::<Vec<_>>(),
        [1, 3]
    );
}

#[test]
fn a_hidden_window_that_missed_a_dropped_update_is_primed_in_full() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let id = Uuid::new_v4();
    let heard = heard_window(&events, "one", id, vec![interest("files", Some(machine))]);
    events.dispatch(update(machine, 1, "files"));
    let mut refresh = events.refresh.subscribe();
    watched::read_and_update(&mut refresh, |_| ());

    events.pause("one", id, None).unwrap();
    events.dispatch(Update {
        data: Some(serde_json::json!("x".repeat(MAX_NODE_SNAPSHOT_BYTES))),
        ..update(machine, 2, "files")
    });

    assert_eq!(events.resume("one", id, None).unwrap(), Some(Primed::Full));
    assert!(refresh.has_changed().unwrap());
    assert_eq!(heard.lock().unwrap().len(), 1);
}

#[test]
fn topics_taken_while_hidden_are_primed_in_full_when_shown() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let _other = heard_window(
        &events,
        "other",
        Uuid::new_v4(),
        vec![interest("files", Some(machine))],
    );
    events.dispatch(update(machine, 1, "files"));
    let id = Uuid::new_v4();
    let heard = heard_window(&events, "one", id, vec![interest("reviews", Some(machine))]);
    events.pause("one", id, None).unwrap();
    events
        .retopic(
            "one",
            id,
            vec![
                interest("reviews", Some(machine)),
                interest("files", Some(machine)),
            ],
        )
        .unwrap();
    assert!(heard.lock().unwrap().is_empty());

    assert_eq!(
        events.resume("one", id, None).unwrap(),
        Some(Primed::Changes)
    );
    assert_eq!(heard.lock().unwrap().len(), 1);
}

#[test]
fn a_reader_that_attaches_again_with_a_sequence_gets_only_newer_updates() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let topics = vec![interest("files", Some(machine))];
    let _other = heard_window(&events, "other", Uuid::new_v4(), topics.clone());
    events.dispatch(update(machine, 1, "files"));
    events.dispatch(update(machine, 2, "reviews"));
    let since = events.registry.lock().unwrap().latest.seq;
    events.dispatch(Update {
        query: Some(1),
        ..update(machine, 3, "files")
    });
    let mut refresh = events.refresh.subscribe();
    watched::read_and_update(&mut refresh, |_| ());
    let heard = Arc::new(Mutex::new(Vec::new()));
    let output = heard.clone();

    let (_stopped, primed) = events
        .register(
            "one",
            Uuid::new_v4(),
            topics,
            Arc::new(move |event| {
                output.lock().unwrap().push(event);
                true
            }),
            || true,
            Some(since),
        )
        .unwrap()
        .unwrap();

    assert_eq!(primed, Primed::Changes);
    assert_eq!(
        heard
            .lock()
            .unwrap()
            .iter()
            .map(|update| update.revision)
            .collect::<Vec<_>>(),
        [3]
    );
    assert!(!refresh.has_changed().unwrap());
}

#[test]
fn the_tray_keeps_the_agents_topic_subscribed_with_every_window_hidden_past_grace() {
    let events = Events::default();
    let id = Uuid::new_v4();
    let _window = heard_window(&events, "one", id, vec![interest("agents", None)]);
    events.registry.lock().unwrap().grace = Duration::ZERO;
    let standing = events.hold("tray", vec![interest("agents", None)]).unwrap();

    assert!(events.pause("one", id, None).unwrap());
    events.changed();
    assert_eq!(
        watched::read_sent(&events.demand, Clone::clone),
        [interest("agents", None)]
    );

    drop(standing);
    assert!(watched::read_sent(&events.demand, |demand| demand.is_empty()));
}

#[test]
fn standing_interests_of_different_holders_stay_apart_and_a_replaced_one_changes_the_demand() {
    let events = Events::default();
    let _tray = events.hold("tray", vec![interest("agents", None)]).unwrap();
    events
        .hold_set("team", vec![interest("sync", None)])
        .unwrap();
    assert_eq!(
        watched::read_sent(&events.demand, Clone::clone).len(),
        2,
        "each holder keeps its own interest"
    );

    events
        .hold_set("team", vec![interest("moves", None)])
        .unwrap();
    assert!(watched::read_sent(&events.demand, |demand| {
        demand.contains(&interest("moves", None)) && !demand.contains(&interest("sync", None))
    }));

    events.hold_set("team", Vec::new()).unwrap();
    assert_eq!(
        watched::read_sent(&events.demand, Clone::clone),
        [interest("agents", None)]
    );
}

#[test]
fn the_tray_standing_interest_keeps_the_stream_on_and_takes_no_updates() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let _standing = events.hold("tray", vec![interest("agents", None)]).unwrap();

    assert!(!events.node_wanted(machine, &Topic::new("agents")));
    events.dispatch(Update {
        machine_id: machine.to_string(),
        topic: Topic::new("agents"),
        revision: 1,
        query: None,
        initial: false,
        data: None,
        seq: 0,
    });
    assert_eq!(events.registry.lock().unwrap().latest.values().count(), 0);
    assert_eq!(
        watched::read_sent(&events.demand, Clone::clone),
        [interest("agents", None)]
    );

    let _window = heard_window(
        &events,
        "one",
        Uuid::new_v4(),
        vec![interest("agents", None)],
    );
    assert!(events.node_wanted(machine, &Topic::new("agents")));
}

#[test]
fn a_standing_interest_in_a_view_has_it_neither_drawn_nor_kept() {
    let events = Events::default();
    let _standing = events
        .hold(
            "notices",
            vec![interest("sync", None), interest("moves", None)],
        )
        .unwrap();

    assert!(!events.controller_wanted(&Topic::new("sync")));
    assert!(!events.controller_held(&Topic::new("moves")));

    let _window = heard_window(&events, "one", Uuid::new_v4(), vec![interest("sync", None)]);
    assert!(events.controller_wanted(&Topic::new("sync")));
    assert!(events.controller_held(&Topic::new("sync")));
    assert!(!events.controller_wanted(&Topic::new("moves")));
}

#[test]
fn a_terminal_list_is_not_taken_for_a_terminals_output() {
    let events = Events::default();
    let machine = iroh::SecretKey::generate().public();
    let output = Interest {
        machine: None,
        topic: Topic::keyed("terminal-output", "7"),
    };
    let _window = register(
        &events,
        "one",
        Uuid::new_v4(),
        vec![output],
        Arc::new(|_| true),
    );

    assert!(!events.node_wanted(machine, &Topic::new("terminals")));
    assert!(events.node_wanted(machine, &Topic::keyed("terminal-output", "7")));
}
