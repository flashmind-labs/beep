use super::*;
use crate::client::SessionFn;
use beep_proto::node::{Open, Opened as NodeOpened};
use beep_proto::watched;
use std::sync::Arc;
use tokio::sync::{Notify, watch};

#[tokio::test(start_paused = true)]
async fn a_start_waits_past_the_call_timeout_for_acps_id() {
    let requested = Arc::new(Notify::new());
    let (ready, waiting) = watch::channel(false);
    let session: SessionFn = {
        let requested = requested.clone();
        Arc::new(move |mut tx, mut rx| {
            let requested = requested.clone();
            let mut waiting = waiting.clone();
            Box::pin(async move {
                rx.recv::<Open>().await.unwrap().unwrap();
                tx.send(&NodeOpened::Ready {}).await.unwrap();
                let first = rx.recv::<Request>().await.unwrap().unwrap();
                assert!(matches!(first, Request::Start { .. }));
                requested.notify_one();
                watched::wait_for(&mut waiting, |ready| *ready)
                    .await
                    .unwrap();
                tx.send(&Event::Attached {
                    session: Box::new(SessionInfo {
                        id: "acp-session".to_owned(),
                        ..SessionInfo::default()
                    }),
                })
                .await
                .unwrap();
            })
        })
    };
    let client = PeerClient::local(session);
    let opening = open_agent(
        &client,
        "codex",
        Request::Start {
            cwd: None,
            prompt: "go".to_owned(),
            model: None,
            effort: None,
            checkout: None,
            images: Vec::new(),
        },
    );
    tokio::pin!(opening);
    tokio::select! {
        _ = requested.notified() => {},
        _ = &mut opening => panic!("startup finished before ACP answered"),
    }
    tokio::time::advance(CALL_TIMEOUT + Duration::from_secs(1)).await;
    assert!(futures::poll!(opening.as_mut()).is_pending());
    ready.send(true).unwrap();
    assert_eq!(opening.await.unwrap().info.id, "acp-session");
}
