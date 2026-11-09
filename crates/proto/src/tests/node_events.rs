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
fn subscriptions_match_a_resource_a_namespace_or_everything() {
    let event = Topic::keyed("files", "/srv/project");
    assert!(event.matches(&event));
    assert!(Topic::new("files").matches(&event));
    assert!(Topic::new("*").matches(&event));
    assert!(!Topic::keyed("files", "/srv/other").matches(&event));
    assert!(!Topic::new("reviews").matches(&event));
    assert!(event.matches(&Topic::new("files")));
    assert!(!Topic::keyed("files", "bad\0path").valid());
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
