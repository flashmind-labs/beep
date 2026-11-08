use super::*;

#[test]
fn local_times_read_in_the_users_zone() {
    let tokyo: Tz = "Asia/Tokyo".parse().expect("a known zone");
    assert_eq!(
        utc("2026-10-12T19:00:00", Some(tokyo)).unwrap(),
        "2026-10-12T10:00:00Z"
    );
    assert_eq!(
        utc("2026-10-12T19:00:00+09:00", None).unwrap(),
        "2026-10-12T10:00:00Z"
    );
    assert!(utc("tomorrow", None).is_err());
}

#[test]
fn a_polyline_decodes_at_its_precision() {
    // Google's own example.
    let points = decode("_p~iF~ps|U_ulLnnqC_mqNvxq`@", 5).unwrap();
    let pairs: Vec<(f64, f64)> = points.iter().map(|p| (p.latitude, p.longitude)).collect();
    assert_eq!(pairs, [(38.5, -120.2), (40.7, -120.95), (43.252, -126.453)]);
    // Cut off inside a number.
    assert!(decode("_p~iF~ps|U_ulL", 5).is_none());
    assert_eq!(decode("", 5).unwrap(), []);
}

#[test]
fn an_overnight_wait_gives_way_to_the_first_train() {
    let leg = |start: &str, end: &str| {
        json!({"mode": "SUBWAY", "from": {"name": "A"}, "to": {"name": "B"}, "duration": 60,
                   "startTime": start, "endTime": end})
    };
    let plan: Plan = serde_json::from_value(json!({
            "itineraries": [
                {"duration": 16320, "startTime": "2026-10-12T15:23:00Z", "endTime": "2026-10-12T19:55:00Z", "transfers": 1,
                 "legs": [leg("2026-10-12T15:23:00Z", "2026-10-12T15:44:00Z"), leg("2026-10-12T19:34:00Z", "2026-10-12T19:55:00Z")]},
                {"duration": 2100, "startTime": "2026-10-12T19:20:00Z", "endTime": "2026-10-12T19:55:00Z", "transfers": 1,
                 "legs": [leg("2026-10-12T19:20:00Z", "2026-10-12T19:30:00Z"), leg("2026-10-12T19:34:00Z", "2026-10-12T19:55:00Z")]},
                {"duration": 2160, "startTime": "2026-10-12T19:30:00Z", "endTime": "2026-10-12T20:06:00Z", "transfers": 1,
                 "legs": [leg("2026-10-12T19:30:00Z", "2026-10-12T19:40:00Z"), leg("2026-10-12T19:45:00Z", "2026-10-12T20:06:00Z")]}
            ]
        }))
        .unwrap();
    let kept: Vec<i64> = useful(&plan.itineraries)
        .iter()
        .map(|r| r.duration)
        .collect();
    assert_eq!(kept, [2100, 2160]);
    assert_eq!(mode_name("REGIONAL_RAIL"), "Regional rail");
}
