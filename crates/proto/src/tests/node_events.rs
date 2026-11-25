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
