use super::*;
use crate::machine::said;
use beep_proto::SessionId;
use beep_proto::node::events::Topic;
use iroh::SecretKey;
use iroh::endpoint::VarInt;
use std::future::pending;

/// A node that pushes the session list and the Codex sign-in state, and is asked nothing.
fn pushing_sessions() -> (Arc<Snapshots>, EndpointId) {
    let snapshots = Arc::new(Snapshots::default());
    let id = SecretKey::generate().public();
    let list = Read::Sessions(Box::new(
        beep_proto::node::agent::SessionsRequest::ListAll {},
    ));
    let sign_in = Read::SignIn {
        tool: "codex".into(),
    };
    snapshots.open_stream(id, list.topic().unwrap());
    snapshots.open_stream(id, sign_in.topic().unwrap());
    snapshots.push(
        id,
        list,
        Value::Sessions(Box::new(beep_proto::node::agent::SessionsReply::Sessions {
            sessions: Vec::new(),
        })),
    );
    snapshots.push(
        id,
        sign_in,
        Value::SignIn(beep_proto::node::sign_in::State::SignedIn {}),
    );

    (snapshots, id)
}

#[tokio::test]
async fn a_subscribed_sign_in_state_uses_its_snapshot_without_opening_an_rpc() {
    let unavailable: SessionFn = Arc::new(|_, _| panic!("a cached read must not open a stream"));
    let (snapshots, id) = pushing_sessions();
    let client = PeerClient::local(unavailable).with_snapshots(snapshots, id);
    assert_eq!(
        crate::machine::sign_in(
            &client,
            &beep_proto::node::sign_in::Request::State {
                tool: "codex".into()
            }
        )
        .await
        .unwrap(),
        beep_proto::node::sign_in::State::SignedIn {}
    );
}

#[tokio::test]
async fn a_subscribed_session_read_uses_its_snapshot_without_opening_an_rpc() {
    let unavailable: SessionFn = Arc::new(|_, _| panic!("a cached read must not open a stream"));
    let (snapshots, id) = pushing_sessions();
    let client = PeerClient::local(unavailable).with_snapshots(snapshots, id);
    assert!(
        crate::machine::sessions::list_all(&client)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A node whose streams are all open, so that a read kept live would show as a query.
fn every_stream_open() -> (Arc<Snapshots>, EndpointId) {
    let snapshots = Arc::new(Snapshots::default());
    let id = SecretKey::generate().public();

    for topic in ["agents", "agent", "files", "moves", "processes"] {
        snapshots.open_stream(id, Topic::new(topic));
    }

    (snapshots, id)
}

#[tokio::test]
async fn found_checkouts_are_asked_once_and_never_kept_live() {
    use beep_proto::node::{
        Open, Opened,
        agent::{SessionsReply, SessionsRequest},
    };

    let node: SessionFn = Arc::new(|mut tx, mut rx| {
        Box::pin(async move {
            rx.recv::<Open>().await.unwrap().unwrap();
            tx.send(&Opened::Ready {}).await.unwrap();
            let asked = rx.recv::<SessionsRequest>().await.unwrap();
            assert_eq!(asked, Some(SessionsRequest::Folders { limit: 7 }));
            tx.send(&SessionsReply::Folders {
                folders: Vec::new(),
            })
            .await
            .unwrap();
        })
    });
    let (snapshots, id) = every_stream_open();
    let client = PeerClient::local(node).with_snapshots(snapshots.clone(), id);
    let found = crate::machine::sessions::found_folders(&client, 7)
        .await
        .unwrap();
    assert!(found.is_empty());
    assert!(snapshots.queries(id).is_empty());
}

/// Reads asked once go to the node as calls and never become live queries:
/// a live query is read again on the node after every matching change.
#[tokio::test]
async fn one_off_reads_are_calls_and_never_kept_live() {
    use crate::machine::sessions::{self, NodeFile, NodeWork, Reading, WorkRead};
    use beep_proto::node::{
        Open, Opened,
        agent::{ExternalAsk, SessionsReply, SessionsRequest},
    };

    let node: SessionFn = Arc::new(|mut tx, mut rx| {
        Box::pin(async move {
            rx.recv::<Open>().await.unwrap().unwrap();
            tx.send(&Opened::Ready {}).await.unwrap();
            let reply = match rx.recv::<SessionsRequest>().await.unwrap().unwrap() {
                SessionsRequest::External { .. } | SessionsRequest::ExternalNamed { .. } => {
                    SessionsReply::External {
                        sessions: Vec::new(),
                    }
                }
                SessionsRequest::ReadFile { .. } => SessionsReply::NoFile {},
                SessionsRequest::Worktrees { .. } => SessionsReply::Worktrees {
                    worktrees: Vec::new(),
                },
                SessionsRequest::Folder { .. } => SessionsReply::Tree {
                    tree: beep_proto::WorkTree {
                        folders: Vec::new(),
                        truncated: false,
                    },
                    changed: Vec::new(),
                },
                SessionsRequest::Commands { .. } => SessionsReply::Commands {
                    commands: Vec::new(),
                },
                other => panic!("{other:?} was not expected"),
            };
            tx.send(&reply).await.unwrap();
        })
    });
    let (snapshots, id) = every_stream_open();
    let client = PeerClient::local(node).with_snapshots(snapshots.clone(), id);

    let one = sessions::external(&client, "claude", 1, Some("example-session".into()));
    assert!(one.await.unwrap().is_empty());
    let asks = vec![ExternalAsk {
        agent: "claude".into(),
        session: "example-session".into(),
    }];
    assert_eq!(
        sessions::external_named(&client, asks).await.unwrap(),
        Some(Vec::new())
    );
    let read = WorkRead {
        root: "/srv/project",
        path: "a.txt",
        offset: 0,
        len: 16,
    };
    assert!(matches!(
        sessions::read_file(&client, read).await.unwrap(),
        NodeFile::Missing
    ));
    assert!(matches!(
        sessions::worktrees(&client, "/srv/project", None)
            .await
            .unwrap(),
        NodeWork::Done(_)
    ));
    assert!(matches!(
        sessions::commands(&client, &SessionId::from("example-session"))
            .await
            .unwrap(),
        NodeWork::Done(_)
    ));
    let list = beep_proto::ListWorkTree {
        machine_id: String::new(),
        root: "/srv/project".into(),
        path: String::new(),
        ignored: true,
        depth: 1,
    };
    assert!(matches!(
        sessions::tree(&client, &list, Reading::Once).await.unwrap(),
        NodeWork::Done(_)
    ));
    assert!(snapshots.queries(id).is_empty());
}

/// A subscription that never gives its snapshot, as while the node sets
/// up a large file watch.
#[tokio::test(start_paused = true)]
async fn a_followed_read_asks_the_node_when_its_subscription_is_slow() {
    use crate::machine::sessions::{self, NodeWork, Reading};
    use beep_proto::node::{
        Open, Opened,
        agent::{SessionsReply, SessionsRequest},
    };

    let node: SessionFn = Arc::new(|mut tx, mut rx| {
        Box::pin(async move {
            rx.recv::<Open>().await.unwrap().unwrap();
            tx.send(&Opened::Ready {}).await.unwrap();
            let asked = rx.recv::<SessionsRequest>().await.unwrap().unwrap();
            assert!(matches!(asked, SessionsRequest::Folder { .. }));
            tx.send(&SessionsReply::Tree {
                tree: beep_proto::WorkTree {
                    folders: Vec::new(),
                    truncated: false,
                },
                changed: Vec::new(),
            })
            .await
            .unwrap();
        })
    });
    let snapshots = Arc::new(Snapshots::default());
    let id = SecretKey::generate().public();
    snapshots.open_stream(id, Topic::keyed("files", "/srv/project"));
    let client = PeerClient::local(node).with_snapshots(snapshots, id);
    let list = beep_proto::ListWorkTree {
        machine_id: String::new(),
        root: "/srv/project".into(),
        path: String::new(),
        ignored: false,
        depth: 1,
    };
    assert!(matches!(
        sessions::tree(&client, &list, Reading::Followed)
            .await
            .unwrap(),
        NodeWork::Done(_)
    ));
}

/// A wait whose subscription fails, then ends, and is then open again with the value.
#[tokio::test(start_paused = true)]
async fn a_wait_opens_its_subscription_again_when_it_fails_or_ends() {
    use beep_proto::node::{events::Value, sync::Reply};

    let status = Value::Sync(Box::new(Reply::Status {
        projects: Vec::new(),
    }));
    let snapshots = Arc::new(Snapshots::default());
    let id = SecretKey::generate().public();
    let topic = Read::SyncList.topic().unwrap();
    let generation = snapshots.open_stream(id, topic.clone());
    let unasked: SessionFn = Arc::new(|_, _| panic!("the status is pushed, not asked"));
    let client = PeerClient::local(unasked).with_snapshots(snapshots.clone(), id);
    let started = tokio::time::Instant::now();

    let wait = client.snapshot_until(Read::SyncList, |value| {
        matches!(value, Value::Sync(_)).then_some(())
    });
    let node = async {
        let mut changes = snapshots.changes();
        let asked = |snapshots: &Snapshots| !snapshots.queries(id).is_empty();

        // The first subscription fails.
        while !asked(&snapshots) {
            changes.changed().await.unwrap();
        }
        snapshots.refuse(id, Read::SyncList, "node failed");

        // The second ends: its stream goes, and is open again for the third.
        loop {
            changes.changed().await.unwrap();
            if asked(&snapshots) {
                break;
            }
        }
        snapshots.end(id, generation);
        snapshots.open_stream(id, topic.clone());

        // The third pushes the value.
        loop {
            changes.changed().await.unwrap();
            if asked(&snapshots) {
                break;
            }
        }
        snapshots.push(id, Read::SyncList, status);
    };

    let (found, ()) = tokio::join!(wait, node);

    assert!(found.is_ok());
    assert_eq!(started.elapsed(), RETRY * 2);
}

fn link(e: impl StdError + Send + Sync + 'static) -> MeshError {
    MeshError::Link(anyhow::Error::new(e))
}

#[test]
fn a_stream_the_node_stopped_or_reset_is_transport() {
    // What a framed write hands back: the QUIC error inside an I/O error.
    let stopped = io::Error::from(WriteError::Stopped(VarInt::from_u32(0)));
    assert_eq!(stopped.to_string(), "sending stopped by peer: error 0");
    assert!(link(stopped).is_transport());
    let reset = io::Error::from(ReadError::Reset(VarInt::from_u32(0)));
    assert!(link(reset).is_transport());
    let lost = anyhow::Error::new(ConnectionError::TimedOut).context("opening");
    assert!(MeshError::Link(lost).is_transport());
    assert!(link(Offline(SecretKey::generate().public())).is_transport());
    assert!(MeshError::Net("not connected".into()).is_transport());
    // `PeerClient::over` wraps the mesh's own error.
    let wrapped = anyhow::Error::new(MeshError::Net("not connected".into()));
    assert!(MeshError::Link(wrapped).is_transport());
}

#[tokio::test(start_paused = true)]
async fn no_answer_in_time_is_transport() {
    let elapsed = timeout(CALL_TIMEOUT, pending::<()>())
        .await
        .expect_err("a pending future times out");
    let e = anyhow::Error::new(elapsed).context("the node did not answer");
    assert!(MeshError::Link(e).is_transport());
}

/// A node in this process that never answers `Open` fails the open in
/// time instead of holding the caller for good (#427).
#[tokio::test(start_paused = true)]
async fn a_local_node_that_never_answers_fails_the_open() {
    let silent: SessionFn = Arc::new(|tx: LinkTx, rx: LinkRx| {
        Box::pin(async move {
            let _held = (tx, rx);
            pending::<()>().await;
        })
    });
    let opened = PeerClient::local(silent)
        .open_sending("codex/1", &"resume")
        .await;
    let Err(e) = opened else {
        panic!("no answer is an error");
    };
    assert!(e.is_transport(), "{e:#}");
}

#[test]
fn what_the_node_said_is_not_transport() {
    assert!(!said("no such session").is_transport());
    assert!(!link(io::Error::other("bad frame")).is_transport());
    let declined = MeshError::Declined {
        protocol: "sessions/1".into(),
        reason: "not served".into(),
    };
    assert!(!declined.is_transport());
    assert!(!MeshError::Unexpected("Pong".into()).is_transport());
}

#[tokio::test]
async fn opening_commands_waits_for_ready_and_consumes_it_once() {
    use beep_proto::node::{Open, Opened, commands};
    use tokio::sync::Notify;

    let started = Arc::new(Notify::new());
    let accepted = Arc::new(Notify::new());
    let session: SessionFn = {
        let started = started.clone();
        let accepted = accepted.clone();
        Arc::new(move |mut tx, mut rx| {
            let started = started.clone();
            let accepted = accepted.clone();
            Box::pin(async move {
                assert_eq!(
                    rx.recv::<Open>().await.unwrap().unwrap().protocol,
                    commands::PROTOCOL
                );
                tx.send(&Opened::Ready {}).await.unwrap();
                assert_eq!(
                    rx.recv::<commands::Open>().await.unwrap(),
                    Some(commands::Open::Calls)
                );
                started.notify_one();
                accepted.notified().await;
                tx.send(&commands::Reply::Ready).await.unwrap();
                tx.send(&commands::Reply::Pong { id: 9 }).await.unwrap();
                let _ = rx.recv::<u8>().await;
            })
        })
    };
    let client = PeerClient::local(session);
    let opening = client.open_commands();
    tokio::pin!(opening);
    timeout(Duration::from_secs(1), async {
        tokio::select! {
            _ = &mut opening => panic!("the stream opened before Ready"),
            _ = started.notified() => {},
        }
    })
    .await
    .unwrap();
    assert!(futures::poll!(&mut opening).is_pending());
    accepted.notify_one();
    let (_tx, mut rx) = timeout(Duration::from_secs(1), opening)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rx.recv::<commands::Reply>().await.unwrap(),
        Some(commands::Reply::Pong { id: 9 })
    );
}

#[tokio::test]
async fn opening_protocols_rejects_a_reply_before_ready() {
    use beep_proto::node::{Open, Opened, commands, subscriptions};

    let session: SessionFn = Arc::new(|mut tx, mut rx| {
        Box::pin(async move {
            let protocol = rx.recv::<Open>().await.unwrap().unwrap().protocol;
            tx.send(&Opened::Ready {}).await.unwrap();
            if protocol == commands::PROTOCOL {
                rx.recv::<commands::Open>().await.unwrap().unwrap();
                tx.send(&commands::Reply::Pong { id: 0 }).await.unwrap();
            } else {
                assert_eq!(protocol, subscriptions::PROTOCOL);
                rx.recv::<subscriptions::Open>().await.unwrap().unwrap();
                tx.send(&subscriptions::Reply::Pong { id: 0 })
                    .await
                    .unwrap();
            }
        })
    });
    let client = PeerClient::local(session);
    assert!(
        client
            .open_commands()
            .await
            .err()
            .expect("a reply before Ready must be rejected")
            .to_string()
            .contains("Ready")
    );
    assert!(
        client
            .open_subscriptions()
            .await
            .err()
            .expect("a reply before Ready must be rejected")
            .to_string()
            .contains("Ready")
    );
}

#[tokio::test(start_paused = true)]
async fn attachment_uploads_allow_more_than_30_seconds_with_progress() {
    use crate::machine::sessions::{self, NodeWork, WorkWrite};
    use beep_proto::node::agent::{SessionsReply, SessionsRequest};
    use tokio::sync::Notify;
    use tokio::time::{Instant, advance};

    let received = Arc::new(Notify::new());
    let accept = Arc::new(Notify::new());
    let accepted = Arc::new(Notify::new());
    let reply = Arc::new(Notify::new());
    let node: SessionFn = {
        let received = received.clone();
        let accept = accept.clone();
        let accepted = accepted.clone();
        let reply = reply.clone();
        Arc::new(move |mut tx, mut rx| {
            let received = received.clone();
            let accept = accept.clone();
            let accepted = accepted.clone();
            let reply = reply.clone();
            Box::pin(async move {
                rx.recv::<Open>().await.unwrap().unwrap();
                assert!(matches!(
                    rx.recv::<SessionsRequest>().await.unwrap(),
                    Some(SessionsRequest::UploadFile { .. })
                ));
                received.notify_one();
                accept.notified().await;
                tx.send(&Opened::Ready {}).await.unwrap();
                accepted.notify_one();
                reply.notified().await;
                tx.send(&SessionsReply::Written {
                    path: "/cache/photo.jpg".into(),
                })
                .await
                .unwrap();
            })
        })
    };
    let client = PeerClient::local(node);
    let write = WorkWrite {
        root: "/srv/project".into(),
        path: "photo.jpg".into(),
        offset: 0,
        size: 3,
        data: vec![1, 2, 3],
    };
    let mut upload = Box::pin(sessions::upload_file(
        &client,
        "codex".into(),
        "session".into(),
        write,
    ));
    let started = Instant::now();

    tokio::select! {
        _ = &mut upload => panic!("the upload needs an acknowledgement"),
        _ = received.notified() => {},
    }
    advance(Duration::from_secs(20)).await;
    accept.notify_one();
    tokio::select! {
        _ = &mut upload => panic!("the upload needs a reply"),
        _ = accepted.notified() => {},
    }
    assert!(futures::poll!(&mut upload).is_pending());
    advance(Duration::from_secs(20)).await;
    reply.notify_one();

    assert!(matches!(upload.await.unwrap(), NodeWork::Done(path) if path == "/cache/photo.jpg"));
    assert_eq!(started.elapsed(), Duration::from_secs(40));
}

#[tokio::test(start_paused = true)]
async fn attachment_uploads_report_30_seconds_without_progress() {
    use crate::machine::sessions::{self, WorkWrite};
    use beep_proto::node::agent::SessionsRequest;
    use tokio::time::Instant;

    let node: SessionFn = Arc::new(|mut tx, mut rx| {
        Box::pin(async move {
            rx.recv::<Open>().await.unwrap().unwrap();
            tx.send(&Opened::Ready {}).await.unwrap();
            rx.recv::<SessionsRequest>().await.unwrap().unwrap();
            let _held = (tx, rx);
            pending::<()>().await;
        })
    });
    let client = PeerClient::local(node);
    let started = Instant::now();
    let result = sessions::upload_file(
        &client,
        "codex".into(),
        "session".into(),
        WorkWrite {
            root: "/srv/project".into(),
            path: "photo.jpg".into(),
            offset: 0,
            size: 3,
            data: vec![1, 2, 3],
        },
    )
    .await;
    let error = result.err().expect("a silent upload must time out");

    assert_eq!(started.elapsed(), Duration::from_secs(30));
    assert!(error.is_transport());
    assert_eq!(
        error.to_string(),
        "the transfer made no progress for 30 seconds"
    );
}
