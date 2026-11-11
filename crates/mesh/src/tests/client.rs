use super::*;
use crate::machine::said;
use iroh::SecretKey;
use iroh::endpoint::VarInt;
use std::future::pending;

struct CurrentSessions;

impl SnapshotReader for CurrentSessions {
    fn snapshot(&self, read: Read) -> SnapshotFuture<'_> {
        Box::pin(async move {
            match read {
                Read::Sessions(request)
                    if matches!(
                        request.as_ref(),
                        beep_proto::node::agent::SessionsRequest::ListAll {}
                    ) =>
                {
                    Some(Ok(Value::Sessions(Box::new(
                        beep_proto::node::agent::SessionsReply::Sessions {
                            sessions: Vec::new(),
                        },
                    ))))
                }
                Read::SignIn { tool } if tool == "codex" => Some(Ok(Value::SignIn(
                    beep_proto::node::sign_in::State::SignedIn {},
                ))),
                _ => None,
            }
        })
    }
}

#[tokio::test]
async fn a_subscribed_sign_in_state_uses_its_snapshot_without_opening_an_rpc() {
    let unavailable: SessionFn = Arc::new(|_, _| panic!("a cached read must not open a stream"));
    let client = PeerClient::local(unavailable).with_snapshot_reader(Arc::new(CurrentSessions));
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
    let client = PeerClient::local(unavailable).with_snapshot_reader(Arc::new(CurrentSessions));
    assert!(
        crate::machine::sessions::list_all(&client)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A reader that would keep every read live: found checkouts must not use it.
struct EveryRead;

impl SnapshotReader for EveryRead {
    fn snapshot(&self, read: Read) -> SnapshotFuture<'_> {
        Box::pin(async move { panic!("{read:?} was kept live") })
    }
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
    let client = PeerClient::local(node).with_snapshot_reader(Arc::new(EveryRead));
    let found = crate::machine::sessions::found_folders(&client, 7)
        .await
        .unwrap();
    assert!(found.is_empty());
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
