use super::*;

#[test]
fn rejects_unbounded_labels_and_invalid_durations() {
    let mut sample = Timing {
        kind: "page".into(),
        name: "agent".into(),
        outcome: "ok".into(),
        ms: 25.0,
    };
    assert_eq!(labels(&sample), Some(("page_mount_painted", "agent", "ok")));
    sample.name = "a private page argument".into();
    assert_eq!(labels(&sample), None);
    sample.name = "agent".into();
    sample.ms = f64::NAN;
    assert_eq!(labels(&sample), None);
    sample.ms = 600_001.0;
    assert_eq!(labels(&sample), None);
}

fn ui_batch(json: serde_json::Value) -> UiBatch {
    serde_json::from_value(json).expect("a valid batch")
}

#[test]
fn merges_ui_batches_into_the_metrics() {
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    let batch = ui_batch(serde_json::json!({
        "window": "main",
        "histograms": [
            { "kind": "interaction", "view": "session", "input": "key",
              "counts": [3, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0], "sum_ms": 1700.0, "max_ms": 900.0 },
            { "kind": "long_task", "view": "home", "input": null,
              "counts": [0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1], "sum_ms": 7250.0, "max_ms": 7000.0 },
            { "kind": "open", "view": "diff", "input": "key",
              "counts": [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "sum_ms": 10.0, "max_ms": 10.0 },
        ],
        "shown_ms": [{ "view": "session", "ms": 59_500.4 }],
        "hidden_ms": 500.0,
        "slow": [],
        "supported": ["event", "long-animation-frame"],
        "overhead_ms": 1.6,
    }));

    metrics::with_local_recorder(&recorder, || record_ui(&batch));
    let text = handle.render();

    let line = |prefix: &str| {
        text.lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("no {prefix} in\n{text}"))
            .to_owned()
    };
    let session = r#"window="main",kind="interaction",view="session",input="key""#;
    assert!(line(&format!("beep_desktop_ui_seconds_count{{{session}}}")).ends_with(" 6"));
    // Three at 8 ms, one at 75 ms and two at 750 ms.
    let sum: f64 = line(&format!("beep_desktop_ui_seconds_sum{{{session}}}"))
        .rsplit_once(' ')
        .and_then(|(_, value)| value.parse().ok())
        .expect("a sum");
    assert!((sum - 1.599).abs() < 1e-9, "{sum}");
    assert!(
        line(r#"beep_desktop_ui_slow_total{window="main",kind="interaction",view="session"}"#)
            .ends_with(" 2")
    );
    assert!(
        line(r#"beep_desktop_ui_slow_total{window="main",kind="long_task",view="home"}"#)
            .ends_with(" 2")
    );
    assert!(
        line(r#"beep_desktop_ui_shown_milliseconds_total{window="main",view="session"}"#)
            .ends_with(" 59500")
    );
    assert!(line(r#"beep_desktop_ui_hidden_milliseconds_total{window="main"}"#).ends_with(" 500"));
    assert!(line(r#"beep_desktop_ui_overhead_milliseconds_total{window="main"}"#).ends_with(" 2"));
    assert!(
        line(r#"beep_desktop_ui_observed{window="main",entry="long_animation_frame"}"#)
            .ends_with(" 1")
    );
    assert!(line(r#"beep_desktop_ui_observed{window="main",entry="longtask"}"#).ends_with(" 0"));
    // An open time cannot carry an input type.
    assert!(!text.contains(r#"view="diff""#));
}

#[test]
fn rejects_ui_labels_outside_the_fixed_sets() {
    let batch = serde_json::json!({
        "window": "main",
        "histograms": [{ "kind": "open", "view": "a private project", "input": null,
            "counts": [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "sum_ms": 1.0, "max_ms": 1.0 }],
        "shown_ms": [], "hidden_ms": 0.0, "slow": [], "supported": [], "overhead_ms": 0.0,
    });
    assert!(serde_json::from_value::<UiBatch>(batch).is_err());

    let mut histogram = UiHistogram {
        kind: UiKind::Open,
        view: UiView::Home,
        input: None,
        counts: vec![0; 11],
        sum_ms: f64::NAN,
        max_ms: 0.0,
    };
    assert!(!histogram.valid());
    histogram.sum_ms = 1.0;
    histogram.counts.push(1);
    assert!(!histogram.valid());
}

#[test]
fn a_phone_writes_its_merged_ui_metrics_as_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let phone = PhoneMetrics::new(recorder.handle(), dir.path()).unwrap();
    let batch = ui_batch(serde_json::json!({
        "window": "phone",
        "histograms": [{ "kind": "interaction", "view": "home", "input": "pointer",
            "counts": [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "sum_ms": 8.0, "max_ms": 8.0 }],
        "shown_ms": [], "hidden_ms": 0.0, "slow": [], "supported": [], "overhead_ms": 0.0,
    }));

    metrics::with_local_recorder(&recorder, || record_ui(&batch));
    write_snapshot(&phone.handle, &phone.file).unwrap();

    let file = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.starts_with("# snapshot unix_seconds="));
    assert!(text.contains(r#"beep_desktop_ui_seconds_count{window="phone",kind="interaction",view="home",input="pointer"} 1"#));
    assert!(text.ends_with("# end snapshot\n"));
}
