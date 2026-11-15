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
    // A long log goes in two messages, as a node sends one in parts.
    let mut log = log;
    let rest = log.split_off(if log.len() > 1024 {
        log.len() / 2
    } else {
        log.len()
    });
    let reply = Reply::Share {
        log,
        status: "running".into(),
        version: "0.1.0".into(),
        os: "linux".into(),
        os_version: "6.1".into(),
    };
    tx.send(&reply).await.ok();

    if !rest.is_empty() {
        tx.send(&Reply::Log { text: rest }).await.ok();
    }
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
async fn a_long_log_arrives_whole() {
    let mut log = String::new();
    let mut n = 0u32;

    while log.len() < 2 * 1024 * 1024 {
        log.push_str(&format!("line {n} of the log\n"));
        n += 1;
    }

    let shares = collect(vec![target("build-box", local(sharing(&log)))], 0).await;
    assert_eq!(shares[0].answer, Answer::Answered);
    assert!(
        shares[0].log == log,
        "{} of {}",
        shares[0].log.len(),
        log.len()
    );
}

#[test]
fn a_local_log_starts_at_the_period() {
    let log = "2026-01-02T03:04:04.999999Z  INFO before\n  its second line\n\
               2026-01-02T03:04:05.000001Z  INFO at\n  its second line\n\
               2026-01-02T03:04:06.000000Z  WARN after\n";
    let since = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
        .expect("a valid time")
        .timestamp();

    let kept = from_time(log.to_owned(), since);
    assert!(
        kept.starts_with("2026-01-02T03:04:05.000001Z  INFO at\n"),
        "{kept}"
    );
    assert!(kept.ends_with("WARN after\n"), "{kept}");
    assert_eq!(from_time(log.to_owned(), since + 60), "");
}
