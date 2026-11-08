use super::*;
use std::time::Duration;

fn fix(place: Option<&str>, updated_at: DateTime<Utc>) -> Location {
    Location {
        latitude: 35.69384,
        longitude: 139.70341,
        accuracy: 12.0,
        timezone: "Asia/Tokyo".into(),
        place: place.map(Into::into),
        updated_at,
    }
}

#[test]
fn location_line_gives_only_coordinates_and_age() {
    let now = Utc::now();
    let loc = fix(
        Some("Shinjuku, Tokyo, Japan"),
        now - Duration::from_secs(180),
    );
    assert_eq!(
        location_line(&loc, now),
        "Location: 35.6938 N, 139.7034 E (3 min ago)"
    );
}
