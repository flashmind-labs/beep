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
        dir.path().join("metrics.2026-01-01.log"),
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
        text.contains("observed 160..280, 3 complete snapshots"),
        "{text}"
    );
    assert!(
        text.contains("actions_total: increase=7 first=15 last=7 resets=1 samples=3"),
        "{text}"
    );
    assert!(text.contains("min=0.5 max=0.5 last=0.5 samples=3"));
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
