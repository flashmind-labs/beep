use super::*;

#[test]
fn live_queries_only_accept_reads() {
    assert!(
        Read::Sessions(Box::new(agent::SessionsRequest::ListAll {}))
            .topic()
            .is_some()
    );
    assert!(
        Read::Sessions(Box::new(agent::SessionsRequest::WriteFile {
            root: "/srv/project".into(),
            path: "file".into(),
            offset: 0,
            size: 0,
            data: vec![]
        }))
        .topic()
        .is_none()
    );
    assert!(
        Read::Process(process::Request::Start {
            command: "false".into(),
            cwd: None
        })
        .topic()
        .is_none()
    );
    assert!(
        Read::Sessions(Box::new(agent::SessionsRequest::FollowPresence {}))
            .topic()
            .is_none()
    );
}

#[test]
fn snapshot_subscriptions_roundtrip_compact_messagepack() {
    let read = Read::Sessions(Box::new(agent::SessionsRequest::Folder {
        root: "/srv/project".into(),
        path: String::new(),
        ignored: false,
        depth: 1,
    }));
    let request = Request::Watch {
        id: 1,
        topics: vec![Topic::parse("files.org.repo").unwrap()],
        queries: vec![Query {
            id: 2,
            topic: read.topic().unwrap(),
            read,
        }],
    };
    let bytes = rmp_serde::to_vec(&request).unwrap();
    assert_eq!(
        crate::lenient::from_slice::<Request>(&bytes).unwrap(),
        request
    );
    let reply = Reply::Snapshot(Box::new(Snapshot {
        topic: Topic::parse("files.org.repo").unwrap(),
        query: Some(2),
        revision: 1,
        value: Value::Missing,
    }));
    assert_eq!(
        crate::lenient::from_slice::<Reply>(&rmp_serde::to_vec(&reply).unwrap()).unwrap(),
        reply
    );
}

#[test]
fn dedicated_protocols_reject_each_others_operations() {
    use crate::node::{commands, subscriptions};
    let call = commands::Request::Call {
        id: 1,
        operation: Box::new(Operation::Process(process::Request::List {})),
    };
    let bytes = rmp_serde::to_vec(&call).unwrap();
    assert!(crate::lenient::from_slice::<subscriptions::Request>(&bytes).is_err());

    let watch = subscriptions::Request::Watch {
        id: 2,
        topic: Topic::new("agents"),
        queries: vec![],
        deltas: false,
    };
    let bytes = rmp_serde::to_vec(&watch).unwrap();
    assert!(crate::lenient::from_slice::<commands::Request>(&bytes).is_err());
    assert_eq!(
        crate::lenient::from_slice::<subscriptions::Request>(&bytes).unwrap(),
        watch
    );
}

/// A request from a newer app: its operation is one this build lacks.
#[derive(Serialize)]
enum NewerRequest {
    #[allow(dead_code)]
    Watch {},
    Call {
        id: u64,
        operation: NewerOperation,
    },
}

#[derive(Serialize)]
enum NewerOperation {
    #[allow(dead_code)]
    Agent {},
    #[allow(dead_code)]
    Sessions {},
    #[allow(dead_code)]
    Process {},
    Teleport {
        to: String,
    },
}

#[test]
fn a_call_with_a_newer_operation_keeps_its_id() {
    let call = NewerRequest::Call {
        id: 7,
        operation: NewerOperation::Teleport {
            to: "build-box".into(),
        },
    };

    for frame in [
        rmp_serde::to_vec(&call).unwrap(),
        rmp_serde::to_vec_named(&call).unwrap(),
    ] {
        assert!(matches!(
            Incoming::read(&frame),
            Ok(Incoming::Unread { id: 7, .. })
        ));
    }
}

#[test]
fn a_known_request_reads_and_garbage_fails() {
    let ping = rmp_serde::to_vec(&Request::Ping { id: 3 }).unwrap();
    assert_eq!(
        Incoming::read(&ping).unwrap(),
        Incoming::Request(Request::Ping { id: 3 })
    );
    assert!(Incoming::read(&[0xc1]).is_err());
}

#[test]
fn team_cards_ride_a_snapshot_after_the_values_before_them() {
    use crate::node::tools::TeamCards;

    for value in [
        Value::ToolUpdates(Vec::new()),
        Value::TeamCards(TeamCards::default()),
    ] {
        let reply = Reply::Snapshot(Box::new(Snapshot {
            topic: Topic::keyed(TEAM_CARDS_TOPIC, "org/repo"),
            query: None,
            revision: 3,
            value,
        }));
        let bytes = rmp_serde::to_vec(&reply).unwrap();

        assert_eq!(crate::lenient::from_slice::<Reply>(&bytes).unwrap(), reply);
    }
}

#[test]
fn team_invites_ride_a_snapshot_after_the_team_cards() {
    let reply = Reply::Snapshot(Box::new(Snapshot {
        topic: Topic::new(TEAM_INVITES_TOPIC),
        query: None,
        revision: 4,
        value: Value::TeamInvites(Vec::new()),
    }));
    let bytes = rmp_serde::to_vec(&reply).unwrap();

    assert_eq!(crate::lenient::from_slice::<Reply>(&bytes).unwrap(), reply);
}

fn running(id: &str, started_at: u64) -> agent::SessionInfo {
    agent::SessionInfo {
        id: id.into(),
        agent: "claude".into(),
        started_at,
        state: agent::State {
            status: agent::Status::Working,
            summary: "Edit a file".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn a_row_ignores_what_changes_during_a_turn() {
    let before = running("a", 1);
    let mut during = before.clone();
    during.tokens = 9_000;
    during.cost_usd = Some(0.5);
    during.context = Some(beep_agents_context(10, 100));
    during.state.files_touched.push("src/main.rs".into());
    during.state.queued_messages.push("next".into());
    during.last_message = "Reading the files.\nThen more text that keeps growing".into();
    let mut earlier = before.clone();
    earlier.last_message = "Reading the files.".into();

    assert_ne!(before, during);
    assert_eq!(earlier.row(), during.row());
    assert_eq!(during.row().reply(), during.reply());
    // What a row shows still changes it.
    let mut doing = before.clone();
    doing.state.summary = "Run the tests".into();
    assert_ne!(before.row(), doing.row());
    // So does a move to another checkout, which the lists show.
    let mut moved = before.clone();
    moved.place = Some(crate::node::agent::WorkPlace {
        folder: "/srv/project/.worktrees/fix".into(),
        main: "/srv/project".into(),
        linked: true,
        branch: Some("fix".into()),
        origin: String::new(),
    });
    assert_ne!(before.row(), moved.row());
}

fn beep_agents_context(used: u64, size: u64) -> crate::AgentContext {
    crate::AgentContext { used, size }
}

#[test]
fn a_delta_takes_a_list_to_the_next() {
    let first = vec![running("a", 1), running("b", 2), running("c", 3)];
    let mut second = first.clone();
    second[1].state.summary = "Run the tests".into();
    second.remove(2);
    second.push(running("d", 4));

    let delta = SessionsDelta::between(&first, &second, 7);
    assert_eq!(delta.after, 7);
    assert_eq!(
        delta
            .changed
            .iter()
            .map(|i| i.id.to_string())
            .collect::<Vec<_>>(),
        ["b", "d"]
    );
    assert_eq!(delta.removed, [crate::SessionId::from("c")]);

    let mut held = first;
    delta.apply(&mut held);
    assert_eq!(held, second);
}

#[test]
fn a_watch_from_before_deltas_asks_for_none() {
    #[derive(Serialize)]
    enum Before {
        Watch {
            id: u64,
            topic: Topic,
            queries: Vec<Query>,
        },
    }
    let bytes = rmp_serde::to_vec(&Before::Watch {
        id: 1,
        topic: Topic::new("agents"),
        queries: vec![],
    })
    .unwrap();

    assert_eq!(
        crate::lenient::from_slice::<crate::node::subscriptions::Request>(&bytes).unwrap(),
        crate::node::subscriptions::Request::Watch {
            id: 1,
            topic: Topic::new("agents"),
            queries: vec![],
            deltas: false,
        }
    );
}

fn agents_snapshot(revision: u64, value: Value) -> Snapshot {
    Snapshot {
        topic: Topic::new("agents"),
        query: None,
        revision,
        value,
    }
}

fn sessions_value(sessions: Vec<agent::SessionInfo>) -> Value {
    Value::Sessions(Box::new(agent::SessionsReply::Sessions { sessions }))
}

#[test]
fn a_follower_rebuilds_the_list_from_deltas_and_refuses_a_gap() {
    let first = vec![running("a", 1), running("b", 2)];
    let mut second = first.clone();
    second[0].state.summary = "Run the tests".into();
    let mut third = second.clone();
    third.push(running("c", 3));
    let mut held = HeldSessions::default();

    let whole = agents_snapshot(1, sessions_value(first.clone()));
    assert_eq!(held.hear(whole.clone()), Some(whole));

    let delta = |after, before: &[agent::SessionInfo], now: &[agent::SessionInfo]| {
        Value::SessionsDelta(Box::new(SessionsDelta::between(before, now, after)))
    };
    let heard = held.hear(agents_snapshot(2, delta(1, &first, &second)));
    assert_eq!(
        heard,
        Some(agents_snapshot(2, sessions_value(second.clone())))
    );
    let heard = held.hear(agents_snapshot(4, delta(3, &second, &third)));
    assert_eq!(heard, None, "revision 3 was never heard");
    let heard = held.hear(agents_snapshot(3, delta(2, &second, &third)));
    assert_eq!(heard, Some(agents_snapshot(3, sessions_value(third))));
}

#[test]
fn a_delta_before_any_list_is_a_gap() {
    let mut held = HeldSessions::default();
    let delta = Value::SessionsDelta(Box::default());

    assert_eq!(held.hear(agents_snapshot(1, delta)), None);
}

#[test]
fn other_snapshots_pass_the_follower_unchanged() {
    let mut held = HeldSessions::default();
    let other = Snapshot {
        topic: Topic::new("sync"),
        query: None,
        revision: 3,
        value: Value::Missing,
    };

    assert_eq!(held.hear(other.clone()), Some(other));
}
