use super::*;
use crate::link::peers::Peers;
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

/// A node in no mesh whose log holds `log`, and the `Node` it hands to
/// `tools::start` after `Peers::new` took its own copy, as `serve` does.
async fn node(log: &str) -> (Peers, Node, tempfile::TempDir) {
    use crate::machine::service::logs::Logs;
    use iroh::{Endpoint, endpoint::presets};
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("beep.log");
    std::fs::write(&path, log).expect("the temp folder is writable");
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .bind()
        .await
        .expect("the endpoint binds");
    let bound = crate::link::Offers {
        log: Logs::from(path),
        ..Default::default()
    }
    .bound(&endpoint, dir.path(), "build-box");
    let peers = Peers::new(bound.clone()).await;
    (peers, Node::of(&bound), dir)
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
    let (_peers, node, _dir) = node("started\nGET /x?token=abc123secret\nlast\n").await;
    let (log, failed) = text(call(&node, &request(Tool::BeepLogs, r#"{"lines":2}"#)).await);
    assert!(!failed, "{log}");
    assert!(log.starts_with("build-box's Beep log, 2 lines of"), "{log}");
    assert!(
        log.contains("[redacted]") && !log.contains("abc123secret"),
        "{log}"
    );
    assert!(!log.contains("started"), "{log}");
    let (status, failed) = text(call(&node, &request(Tool::BeepStatus, "{}")).await);
    assert!(!failed, "{status}");
    assert!(status.starts_with("## build-box (node, "), "{status}");
    assert!(status.contains("): answered\n"), "{status}");
    let (other, failed) =
        text(call(&node, &request(Tool::BeepLogs, r#"{"machine":"gpu-box"}"#)).await);
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
    let (_peers, node, _dir) = node("").await;

    for period in ["1h", "4h", "1d", "7d", "1w", "1.5h", "1h 30m"] {
        assert!(
            metrics(
                &node,
                MetricsArgs {
                    machine: None,
                    period: Some(period.to_owned()),
                    ..MetricsArgs::default()
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
                &node,
                MetricsArgs {
                    machine: None,
                    period: Some(period.to_owned()),
                    ..MetricsArgs::default()
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
    let (_peers, node, dir) = node("").await;
    let stamp = now() - 60;
    std::fs::write(dir.path().join("metrics.log"), format!(
        "# snapshot unix_seconds={stamp}\n# TYPE actions_total counter\nactions_total 3\n# end snapshot\n"
    )).unwrap();
    let (summary, failed) =
        text(call(&node, &request(Tool::BeepMetrics, r#"{"period":"4h"}"#)).await);
    assert!(!failed, "{summary}");
    assert!(summary.contains("1 complete snapshots"), "{summary}");
    assert!(summary.contains("actions_total"));
    let (_, failed) = text(call(&node, &request(Tool::BeepMetrics, r#"{"period":"8d"}"#)).await);
    assert!(failed);
}

#[test]
fn log_query_arguments_validate_bounds_and_patterns() {
    let parse = |value| serde_json::from_str::<LogsArgs>(value).unwrap().query();
    assert!(parse("{}").unwrap().is_none());

    for value in [
        r#"{"start":"2026-01-02T00:00:00"}"#,
        r#"{"start":"2026-01-02T00:00:00+01:00"}"#,
        r#"{"start":"2026-01-02T00:00:00Z","minutes":1}"#,
        r#"{"start":"2026-01-02T00:00:00Z","end":"2026-01-02T00:00:00Z"}"#,
        r#"{"start":"2026-01-02T00:00:00Z","end":"2026-01-10T00:00:00Z"}"#,
        r#"{"offset":100001}"#,
        r#"{"regex":true}"#,
        r#"{"search":"[","regex":true}"#,
    ] {
        assert!(parse(value).is_err(), "{value}");
    }
    let query = parse(r#"{"end":"2026-01-02T01:00:00Z","minutes":30,"search":"["}"#)
        .unwrap()
        .unwrap();
    assert_eq!(query.start, "2026-01-02T00:30:00.000000000Z");
    assert!(!query.regex);
    assert!(!query.allow_untimed);
}

#[tokio::test]
async fn log_queries_filter_before_paginating_and_redact_secrets() {
    let log = "2026-01-02T00:00:00Z INFO app: retry one\n\
        2026-01-02T00:00:01Z INFO app: unrelated\n\
        2026-01-02T00:00:02Z INFO app: retry two token=abc123secret\n\
        2026-01-02T00:00:03Z INFO app: retry three\n";
    let (_peers, node, _dir) = node(log).await;
    let args = r#"{"start":"2026-01-02T00:00:00Z","end":"2026-01-02T00:00:04Z","search":"retry (two|three)","regex":true,"lines":1,"offset":1}"#;
    let (log, failed) = text(call(&node, &request(Tool::BeepLogs, args)).await);
    assert!(!failed, "{log}");
    assert!(log.contains("retry two"), "{log}");
    assert!(!log.contains("retry three"), "{log}");
    assert!(
        log.contains("[redacted]") && !log.contains("abc123secret"),
        "{log}"
    );
}

#[test]
fn metrics_query_arguments_validate_bounds_and_search() {
    let parse = |value| {
        serde_json::from_str::<MetricsArgs>(value)
            .unwrap()
            .query(Duration::from_secs(3600))
    };
    assert!(parse("{}").unwrap().is_none());

    for value in [
        r#"{"start":"2026-01-02T00:00:00Z","period":"1h"}"#,
        r#"{"start":"2026-01-02T00:00:00+01:00"}"#,
        r#"{"regex":true}"#,
        r#"{"search":"[","regex":true}"#,
        r#"{"limit":0}"#,
        r#"{"offset":2049}"#,
        r#"{"start":"1969-12-31T23:59:59Z","end":"1970-01-01T00:00:01Z"}"#,
    ] {
        assert!(parse(value).is_err(), "{value}");
    }
    let query = parse(r#"{"end":"2026-01-02T01:00:00Z","search":"quantile=\"0.99\""}"#)
        .unwrap()
        .unwrap();
    assert_eq!(query.start, "2026-01-02T00:00:00.000000000Z");
    assert_eq!(query.limit, 200);
    assert!(!query.regex);
}

#[tokio::test]
async fn metrics_tool_queries_utc_windows_and_paginated_matches() {
    let (_peers, node, dir) = node("").await;
    std::fs::write(
        dir.path().join("metrics.log"),
        "# snapshot unix_seconds=100\nalpha_total 3\nbeta_total 4\ngamma_total 5\n# end snapshot\n",
    )
    .unwrap();
    let args = r#"{"start":"1970-01-01T00:01:40Z","end":"1970-01-01T00:02:00Z","search":"^(alpha|gamma)_","regex":true,"limit":1,"offset":1}"#;
    let (summary, failed) = text(call(&node, &request(Tool::BeepMetrics, args)).await);
    assert!(!failed, "{summary}");
    assert!(summary.contains("gamma_total:"), "{summary}");
    assert!(
        !summary.contains("alpha_total:") && !summary.contains("beta_total:"),
        "{summary}"
    );
    assert!(
        summary.contains("Matching series: 2; offset: 1; returned: 1; next_offset: none"),
        "{summary}"
    );
}
