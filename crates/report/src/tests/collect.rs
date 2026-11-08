use super::*;
use beep_mesh::SessionFn;
use beep_proto::node::{LinkRx, LinkTx, Open, Opened, logs, report};
use iroh::SecretKey;
use std::future::pending;
use std::sync::Arc;
use tokio::time::Instant;

async fn share(mut tx: LinkTx, mut rx: LinkRx, log: String) {
    let Ok(Some(open)) = rx.recv::<Open>().await else {
        return;
    };
    if open.protocol != report::PROTOCOL {
        let reason = format!("this node does not offer {}", open.protocol);
        tx.send(&Opened::Refused { reason }).await.ok();
        return;
    }
    tx.send(&Opened::Ready {}).await.ok();
    let Ok(Some(Request { .. })) = rx.recv::<Request>().await else {
        return;
    };
    let reply = Reply::Share {
        log,
        status: "running".into(),
        version: "0.1.0".into(),
        os: "linux".into(),
        os_version: "6.1".into(),
    };
    tx.send(&reply).await.ok();
}

async fn silent(mut tx: LinkTx, mut rx: LinkRx) {
    rx.recv::<Open>().await.ok();
    tx.send(&Opened::Ready {}).await.ok();
    rx.recv::<Request>().await.ok();
    pending::<()>().await;
}

async fn older(mut tx: LinkTx, mut rx: LinkRx) {
    let Ok(Some(open)) = rx.recv::<Open>().await else {
        return;
    };
    if open.protocol != logs::PROTOCOL {
        let reason = format!("this node does not offer {}", open.protocol);
        tx.send(&Opened::Refused { reason }).await.ok();
        return;
    }
    tx.send(&Opened::Ready {}).await.ok();
    rx.recv::<logs::Request>().await.ok();
    let reply = logs::Reply::Log {
        text: "a\nb\n".into(),
    };
    tx.send(&reply).await.ok();
}

fn sharing(log: &str) -> SessionFn {
    let log = log.to_owned();
    Arc::new(move |tx, rx| Box::pin(share(tx, rx, log.clone())))
}

fn target(name: &str, client: Result<PeerClient, String>) -> Target {
    Target {
        id: SecretKey::generate().public(),
        name: name.into(),
        kind: DeviceKind::Node,
        client,
    }
}

fn local(session: SessionFn) -> Result<PeerClient, String> {
    Ok(PeerClient::local(session))
}

#[tokio::test]
async fn each_machine_gives_its_share() {
    let targets = vec![
        target("build-box", local(sharing("one\n"))),
        target("gpu-box", local(sharing("two\n"))),
    ];
    let shares = collect(targets, 0).await;
    assert_eq!(shares.len(), 2);
    assert_eq!(shares[0].name, "build-box");
    assert_eq!(shares[0].answer, Answer::Answered);
    assert_eq!(shares[0].log, "one\n");
    assert_eq!(shares[0].version, "0.1.0");
    assert_eq!(shares[1].name, "gpu-box");
    assert_eq!(shares[1].answer, Answer::Answered);
    assert_eq!(shares[1].log, "two\n");
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_never_answers_is_silent_and_does_not_hold_up_the_rest() {
    let targets = vec![
        target(
            "build-box",
            local(Arc::new(|tx, rx| Box::pin(silent(tx, rx)))),
        ),
        target("gpu-box", local(sharing("two\n"))),
    ];
    let start = Instant::now();
    let shares = collect(targets, 0).await;
    assert!(start.elapsed() <= ASK_LIMIT + Duration::from_secs(2));
    assert_eq!(shares[0].answer, Answer::Silent);
    assert_eq!(shares[1].answer, Answer::Answered);
    assert_eq!(shares[1].log, "two\n");
}

#[tokio::test]
async fn an_older_node_falls_back_to_its_log() {
    let targets = vec![target(
        "build-box",
        local(Arc::new(|tx, rx| Box::pin(older(tx, rx)))),
    )];
    let shares = collect(targets, 0).await;
    assert_eq!(shares[0].answer, Answer::OlderNode);
    assert_eq!(shares[0].log, "a\nb\n");
}

#[tokio::test]
async fn an_unreachable_machine_is_listed_offline() {
    let targets = vec![target("build-box", Err("offline".into()))];
    let shares = collect(targets, 0).await;
    assert_eq!(
        shares[0].answer,
        Answer::Offline {
            reason: "offline".into()
        }
    );
}

#[tokio::test]
async fn an_oversized_log_keeps_its_newest_lines() {
    let mut log = String::new();
    let mut n = 0u32;
    while log.len() < 2 * 1024 * 1024 {
        log.push_str(&format!("line {n} of the log\n"));
        n += 1;
    }
    let last = format!("line {} of the log\n", n - 1);
    let shares = collect(vec![target("build-box", local(sharing(&log)))], 0).await;
    let cut = &shares[0].log;
    assert!(cut.len() <= MAX_LOG, "{}", cut.len());
    assert!(cut.len() > 1024 * 1024 - 64, "{}", cut.len());
    assert!(cut.starts_with("line "), "{:?}", &cut[..20]);
    assert!(cut.ends_with(&last));
}
