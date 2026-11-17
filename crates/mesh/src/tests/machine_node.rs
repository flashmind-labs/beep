use super::*;

#[test]
fn the_newest_is_neither_installed_nor_restarted() {
    let done = updated(update::Event::Newest {
        version: "0.1.24".into(),
    })
    .expect("an answer");
    assert_eq!((done.from.as_str(), done.to.as_str()), ("0.1.24", "0.1.24"));
    assert!(!done.restarting);
}

#[test]
fn a_failure_keeps_the_nodes_reason() {
    let e = updated(update::Event::Failed {
        reason: "no build for this machine".into(),
    })
    .expect_err("a failure");
    assert_eq!(e.to_string(), "no build for this machine");
}

#[tokio::test]
async fn updates_send_the_selected_channel_and_preserve_legacy_requests() {
    use beep_proto::node::{Open, Opened};
    use std::sync::Arc;

    for (channel, expected) in [
        (
            update::ReleaseChannel::Minor,
            update::Request::Minor { now: false },
        ),
        (
            update::ReleaseChannel::All,
            update::Request::Update { now: false },
        ),
    ] {
        let client = PeerClient::local(Arc::new(move |mut tx, mut rx| {
            let expected = expected.clone();
            Box::pin(async move {
                let open = rx.recv::<Open>().await.expect("open").expect("message");
                assert_eq!(open.protocol, update::PROTOCOL);
                tx.send(&Opened::Ready {}).await.expect("ready");
                assert_eq!(
                    rx.recv::<update::Request>().await.expect("request"),
                    Some(expected)
                );
                tx.send(&update::Event::Newest {
                    version: "0.8.0".into(),
                })
                .await
                .expect("answer");
            })
        }));
        let result = update_node(&client, false, channel).await.expect("update");
        assert_eq!(result.to, "0.8.0");
        assert!(!result.restarting);
    }
}

#[tokio::test]
async fn an_older_node_rejects_minor_updates_without_an_unrestricted_retry() {
    use beep_proto::node::{Open, Opened};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let served = calls.clone();
    let client = PeerClient::local(Arc::new(move |mut tx, mut rx| {
        served.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            rx.recv::<Open>().await.expect("open");
            tx.send(&Opened::Ready {}).await.expect("ready");
            #[derive(serde::Deserialize)]
            enum Older {
                Update { _now: bool },
            }
            assert!(rx.recv::<Older>().await.is_err());
        })
    }));
    let error = update_node(&client, false, update::ReleaseChannel::Minor)
        .await
        .expect_err("unsupported request");
    assert!(error.to_string().contains("beep-node update"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
