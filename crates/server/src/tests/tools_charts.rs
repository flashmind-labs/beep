use super::*;

fn args(json: Value) -> ChartArgs {
    serde_json::from_value(json).expect("test arguments are valid")
}

#[test]
fn time_points_parse_sort_and_drop_bad_ones() {
    let mut dropped = Vec::new();
    let chart = build(
        args(json!({
            "title": "Users",
            "series": [{ "name": "", "points": [
                { "t": "2026-10-02", "v": 2.0 },
                { "t": "2026-10-01T12:00:00Z", "v": 1.0 },
                { "t": "yesterday", "v": 3.0 },
                { "t": "2026-10", "v": 4.0 }
            ] }]
        })),
        &mut dropped,
    )
    .expect("a valid chart");
    assert_eq!(chart.x, ChartAxis::Time);
    assert_eq!(chart.kind, ChartKind::Line);
    assert_eq!(chart.series[0].name, "Series 1");
    let xs: Vec<f64> = chart.series[0].points.iter().map(|p| p.x).collect();
    assert_eq!(
        xs,
        [
            1_790_812_800_000.0,
            1_790_856_000_000.0,
            1_790_899_200_000.0
        ]
    );
    assert_eq!(dropped.len(), 1, "{dropped:?}");
}
