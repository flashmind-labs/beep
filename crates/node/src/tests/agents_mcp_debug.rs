use super::*;
use beep_mesh::DeviceKind;
use iroh::SecretKey;

fn share(answer: Answer) -> Share {
    Share {
        id: SecretKey::generate().public(),
        name: "build-box".into(),
        kind: DeviceKind::Node,
        answer,
        log: String::new(),
        status: String::new(),
        version: "0.4.1".into(),
        os: "linux".into(),
        os_version: "6.8".into(),
    }
}

#[test]
fn a_heading_names_the_machine_and_how_it_answered() {
    assert_eq!(
        heading(&share(Answer::Answered)),
        "## build-box (node, linux 6.8, Beep 0.4.1): answered"
    );
    let mut offline = share(Answer::Offline {
        reason: "offline".into(),
    });
    offline.version.clear();
    offline.os.clear();
    offline.os_version.clear();
    assert_eq!(heading(&offline), "## build-box (node): offline");
}

/// A node in no mesh whose log holds `log`, and its offers.
async fn node(log: &str) -> (Peers, Offers, tempfile::TempDir) {
    use crate::machine::service::logs::Logs;
    use iroh::{Endpoint, endpoint::presets};
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("beep.log");
    std::fs::write(&path, log).expect("the temp folder is writable");
    let offers = Offers {
        log: Logs::from(path),
        ..Offers::default()
    };
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the endpoint binds");
    let peers = Peers::new(
        endpoint,
        dir.path().to_owned(),
        "build-box".into(),
        offers.clone(),
    );
    (peers, offers, dir)
}

fn request(tool: Tool, args: &str) -> Request {
    Request {
        session: "a3f9c1".into(),
        tool,
        args: args.into(),
        approved: Default::default(),
    }
}

fn text(reply: Reply) -> (String, bool) {
    match reply {
        Reply::Done { text, failed } => (text, failed),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_tools_read_this_node_with_secrets_removed() {
    let (_peers, offers, _dir) = node("started\nGET /x?token=abc123secret\nlast\n").await;
    let (log, failed) = text(call(&offers, &request(Tool::BeepLogs, r#"{"lines":2}"#)).await);
    assert!(!failed, "{log}");
    assert!(log.starts_with("build-box's Beep log, 2 lines of"), "{log}");
    assert!(
        log.contains("[redacted]") && !log.contains("abc123secret"),
        "{log}"
    );
    assert!(!log.contains("started"), "{log}");
    let (status, failed) = text(call(&offers, &request(Tool::BeepStatus, "{}")).await);
    assert!(!failed, "{status}");
    assert!(status.starts_with("## build-box (node, "), "{status}");
    assert!(status.contains("): answered\n"), "{status}");
    let (other, failed) = text(
        call(
            &offers,
            &request(Tool::BeepLogs, r#"{"machine":"gpu-box"}"#),
        )
        .await,
    );
    assert!(failed);
    assert_eq!(other, "There is no machine gpu-box.");
}

#[test]
fn a_machine_is_named_as_list_machines_names_it() {
    let target = Target {
        id: SecretKey::generate().public(),
        name: "Build Box".into(),
        kind: DeviceKind::Node,
        client: Err("offline".into()),
    };
    assert!(named(&target, "build-box"));
    assert!(named(&target, &target.id.to_string()));
    assert!(!named(&target, "gpu-box"));
}

#[tokio::test]
async fn metric_periods_accept_human_durations_and_enforce_retention() {
    let (peers, _offers, _dir) = node("").await;

    for period in ["1h", "4h", "1d", "7d", "1w", "1.5h", "1h 30m"] {
        assert!(
            metrics(
                &peers,
                MetricsArgs {
                    machine: None,
                    period: Some(period.to_owned())
                }
            )
            .await
            .is_ok(),
            "{period}"
        );
    }

    for period in [
        "",
        "0h",
        "8d",
        "7d 1ns",
        "-1h",
        "1",
        "1ms",
        "999999999999999999d",
        "日",
    ] {
        assert!(
            metrics(
                &peers,
                MetricsArgs {
                    machine: None,
                    period: Some(period.to_owned())
                }
            )
            .await
            .is_err(),
            "{period}"
        );
    }
}

#[tokio::test]
async fn metrics_tool_reads_retained_snapshots_and_rejects_invalid_periods() {
    let (_peers, offers, dir) = node("").await;
    let stamp = now() - 60;
    std::fs::write(dir.path().join("metrics.log"), format!(
        "# snapshot unix_seconds={stamp}\n# TYPE actions_total counter\nactions_total 3\n# end snapshot\n"
    )).unwrap();
    let (summary, failed) =
        text(call(&offers, &request(Tool::BeepMetrics, r#"{"period":"4h"}"#)).await);
    assert!(!failed, "{summary}");
    assert!(summary.contains("1 complete snapshots"), "{summary}");
    assert!(summary.contains("actions_total"));
    let (_, failed) = text(call(&offers, &request(Tool::BeepMetrics, r#"{"period":"8d"}"#)).await);
    assert!(failed);
}
