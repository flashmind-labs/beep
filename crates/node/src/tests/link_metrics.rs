use super::*;

fn snapshot(time: u64, count: u64) -> String {
    format!(
        "# snapshot unix_seconds={time}\n# TYPE actions counter\nactions_total {count}\nduration_seconds{{quantile=\"0.99\"}} 0.5\n# end snapshot\n"
    )
}

#[test]
fn summarizes_rotations_resets_and_only_complete_in_range_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("metrics.1970-01-01.log"),
        snapshot(100, 10) + &snapshot(160, 15),
    )
    .unwrap();
    let path = dir.path().join("metrics.log");
    std::fs::write(
        &path,
        snapshot(220, 2) + &snapshot(280, 7) + "# snapshot unix_seconds=300\nactions_total 900\n",
    )
    .unwrap();
    let text = summarize(
        &path,
        &Request {
            since: 130,
            until: 300,
        },
    )
    .unwrap();
    assert!(
        text.contains(
            "observed 1970-01-01T00:02:40Z to 1970-01-01T00:04:40Z, 3 complete snapshots"
        ),
        "{text}"
    );
    assert!(
        text.contains("actions_total: increase=7 first=15 last=7 resets=1 samples=3"),
        "{text}"
    );
    assert!(text.contains("min=0.5 max=0.5 last=0.5 samples=3"));
}

#[test]
fn a_short_query_does_not_scan_unrelated_days() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metrics.log");
    let old = dir.path().join("metrics.1970-01-01.log");
    let matching = dir.path().join("metrics.1970-01-02.1.log");
    let before = dir.path().join("metrics.1970-01-02.log");
    let next = dir.path().join("metrics.1970-01-03.log");
    let far_later = dir.path().join("metrics.1970-01-04.log");
    File::create(&old).unwrap().set_len(MAX_SCAN + 1).unwrap();
    std::fs::write(&before, snapshot(86_410, 5)).unwrap();
    std::fs::write(&matching, snapshot(86_420, 6)).unwrap();
    std::fs::write(&next, snapshot(172_790, 7)).unwrap();
    std::fs::write(&path, snapshot(172_801, 8)).unwrap();
    File::create(&far_later)
        .unwrap()
        .set_len(MAX_SCAN + 1)
        .unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_SCAN + 1)
        .unwrap();

    let query = Query {
        start: "1970-01-02T00:00:00Z".into(),
        end: "1970-01-03T00:00:00Z".into(),
        ..metrics_query()
    };
    let text = summarize_query(&path, &query).unwrap();
    assert!(
        text.contains(
            "observed 1970-01-02T00:00:10Z to 1970-01-02T23:59:50Z, 3 complete snapshots"
        )
    );
    assert!(text.contains("increase=2 first=5 last=7 resets=0 samples=3"));

    let text = summarize(
        &path,
        &Request {
            since: 86_400,
            until: 172_799,
        },
    )
    .unwrap();
    assert!(
        text.contains(
            "observed 1970-01-02T00:00:10Z to 1970-01-02T23:59:50Z, 3 complete snapshots"
        )
    );

    let broad = Query {
        start: "1970-01-01T00:00:00Z".into(),
        ..query
    };
    assert!(
        summarize_query(&path, &broad)
            .unwrap_err()
            .to_string()
            .contains("256 MiB")
    );

    std::fs::write(&path, snapshot(172_801, 8)).unwrap();
    let live = Query {
        start: "1970-01-03T00:00:00Z".into(),
        end: "1970-01-03T01:00:00Z".into(),
        ..broad
    };
    let text = summarize_query(&path, &live).unwrap();
    assert!(
        text.contains(
            "observed 1970-01-03T00:00:01Z to 1970-01-03T00:00:01Z, 1 complete snapshots"
        )
    );
}

#[test]
fn rejects_invalid_periods_and_reports_missing_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metrics.log");
    for request in [
        Request { since: 2, until: 1 },
        Request {
            since: 0,
            until: MAX_PERIOD + 1,
        },
    ] {
        assert!(summarize(&path, &request).is_err());
    }
    let text = summarize(
        &path,
        &Request {
            since: 1,
            until: 60,
        },
    )
    .unwrap();
    assert!(text.contains("No complete metrics snapshots"));
}

fn metrics_query() -> Query {
    Query {
        start: "1970-01-01T00:01:40Z".into(),
        end: "1970-01-01T00:04:40Z".into(),
        search: None,
        regex: false,
        limit: 200,
        offset: 0,
    }
}

#[test]
fn query_bounds_are_inclusive_exclusive_and_preserve_counter_calculations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metrics.log");
    std::fs::write(
        &path,
        snapshot(100, 10) + &snapshot(160, 15) + &snapshot(220, 2) + &snapshot(280, 90),
    )
    .unwrap();
    let mut query = metrics_query();
    query.search = Some("^actions_".into());
    query.regex = true;
    let text = summarize_query(&path, &query).unwrap();
    assert!(
        text.contains(
            "observed 1970-01-01T00:01:40Z to 1970-01-01T00:03:40Z, 3 complete snapshots"
        ),
        "{text}"
    );
    assert!(
        text.contains("actions_total: increase=7 first=10 last=2 resets=1 samples=3"),
        "{text}"
    );
    assert!(!text.contains("duration_seconds"), "{text}");

    query.start = "1970-01-01T00:02:40.001Z".into();
    let text = summarize_query(&path, &query).unwrap();
    assert!(
        text.contains(
            "observed 1970-01-01T00:03:40Z to 1970-01-01T00:03:40Z, 1 complete snapshots"
        ),
        "{text}"
    );
    query.end = "1970-01-01T00:03:40.001Z".into();
    assert!(
        summarize_query(&path, &query)
            .unwrap()
            .contains("observed 1970-01-01T00:03:40Z to 1970-01-01T00:03:40Z")
    );
}

#[test]
fn metric_queries_filter_labels_and_paginate_series_in_sorted_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metrics.log");
    std::fs::write(&path, snapshot(100, 10) + &snapshot(160, 15)).unwrap();
    let mut query = metrics_query();
    query.limit = 1;
    let first = summarize_query(&path, &query).unwrap();
    assert!(
        first.contains("Matching series: 2; offset: 0; returned: 1; next_offset: 1"),
        "{first}"
    );
    assert!(first.contains("actions_total:"), "{first}");
    assert!(!first.contains("duration_seconds{"), "{first}");
    query.offset = 1;
    let second = summarize_query(&path, &query).unwrap();
    assert!(second.contains("next_offset: none"), "{second}");
    assert!(!second.contains("actions_total:"), "{second}");
    assert!(second.contains("duration_seconds{"), "{second}");
    query.offset = 0;
    query.search = Some("quantile=\"0.99\"".into());
    let filtered = summarize_query(&path, &query).unwrap();
    assert!(filtered.contains("Matching series: 1"), "{filtered}");
    assert!(filtered.contains("duration_seconds{"), "{filtered}");
    query.offset = 2;
    let empty = summarize_query(&path, &query).unwrap();
    assert!(empty.contains("returned: 0; next_offset: none"), "{empty}");
}

#[test]
fn metric_queries_filter_before_the_series_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metrics.log");
    let mut snapshot = String::from("# snapshot unix_seconds=100\nselected_total 7\n");

    for index in 0..2500 {
        snapshot.push_str(&format!("unrelated_{index} 1\n"));
    }
    snapshot.push_str("# end snapshot\n");
    std::fs::write(&path, snapshot).unwrap();
    let query = Query {
        search: Some("selected".into()),
        ..metrics_query()
    };
    assert!(
        summarize_query(&path, &query)
            .unwrap()
            .contains("selected_total:")
    );
}

#[test]
fn metric_queries_reject_invalid_bounds_pagination_and_patterns() {
    let valid = metrics_query();
    for query in [
        Query {
            start: "1970-01-01T00:01:40+01:00".into(),
            ..valid.clone()
        },
        Query {
            end: valid.start.clone(),
            ..valid.clone()
        },
        Query {
            end: "1970-01-10T00:01:40Z".into(),
            ..valid.clone()
        },
        Query {
            regex: true,
            ..valid.clone()
        },
        Query {
            search: Some("[".into()),
            regex: true,
            ..valid.clone()
        },
        Query {
            limit: 0,
            ..valid.clone()
        },
        Query {
            offset: 2049,
            ..valid.clone()
        },
    ] {
        assert!(Selection::try_from(&query).is_err(), "{query:?}");
    }
}

#[test]
fn metric_queries_reject_oversized_pages_without_silently_skipping_series() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("metrics.log");
    let mut snapshot = String::from("# snapshot unix_seconds=100\n");

    for index in 0..300 {
        snapshot.push_str(&format!(
            "sample_{index}{{label=\"{}\"}} 1\n",
            "x".repeat(1000)
        ));
    }
    snapshot.push_str("# end snapshot\n");
    std::fs::write(&path, snapshot).unwrap();
    let query = Query {
        limit: 300,
        ..metrics_query()
    };
    assert!(
        summarize_query(&path, &query)
            .unwrap_err()
            .to_string()
            .contains("smaller limit")
    );
}
