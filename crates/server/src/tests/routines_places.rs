use super::*;

fn gym(radius: u32) -> SavedPlace {
    SavedPlace {
        id: Uuid::nil(),
        name: "Gym".to_owned(),
        latitude: 41.4,
        longitude: 2.2,
        radius,
        address: None,
        created_at: Utc::now(),
        inside: false,
        last_here_at: None,
        routines: 0,
    }
}

/// A fix `metres` north of the gym.
fn north(metres: f64, accuracy: f64) -> Location {
    Location {
        latitude: 41.4 + metres / 111_195.0,
        longitude: 2.2,
        accuracy,
        timezone: "Europe/Madrid".to_owned(),
        place: None,
        updated_at: Utc::now(),
    }
}

#[test]
fn arriving_needs_a_fix_inside_the_radius() {
    let place = gym(100);
    assert_eq!(
        crossing(&place, false, &north(80.0, 20.0)),
        Some(PlaceEvent::Arrive)
    );
    assert_eq!(crossing(&place, false, &north(130.0, 20.0)), None);
    // Too vague to say.
    assert_eq!(crossing(&place, false, &north(10.0, 500.0)), None);
    // Already there.
    assert_eq!(crossing(&place, true, &north(10.0, 20.0)), None);
}

#[test]
fn leaving_needs_a_fix_clearly_outside() {
    let place = gym(100);
    // Past the radius, but inside the margin of 50 m.
    assert_eq!(crossing(&place, true, &north(140.0, 10.0)), None);
    // Past the margin, but the fix could still be inside.
    assert_eq!(crossing(&place, true, &north(160.0, 80.0)), None);
    assert_eq!(
        crossing(&place, true, &north(160.0, 20.0)),
        Some(PlaceEvent::Leave)
    );
    assert_eq!(
        crossing(&place, true, &north(5000.0, 1000.0)),
        Some(PlaceEvent::Leave)
    );
    // A big place leaves at half its radius past the edge.
    let park = gym(400);
    assert_eq!(crossing(&park, true, &north(590.0, 10.0)), None);
    assert_eq!(
        crossing(&park, true, &north(610.0, 10.0)),
        Some(PlaceEvent::Leave)
    );
}

#[test]
fn place_triggers_read_back() {
    let place = Uuid::new_v4();
    for trigger in [
        PlaceTrigger {
            event: PlaceEvent::Arrive,
            place,
            repeat: PlaceRepeat::Daily,
        },
        PlaceTrigger {
            event: PlaceEvent::Leave,
            place,
            repeat: PlaceRepeat::Always,
        },
        PlaceTrigger {
            event: PlaceEvent::Leave,
            place,
            repeat: PlaceRepeat::Once,
        },
    ] {
        assert_eq!(
            trigger.to_string().parse::<PlaceTrigger>().ok(),
            Some(trigger)
        );
    }
    assert_eq!("0 9 * * *".parse::<PlaceTrigger>().ok(), None);
    assert_eq!(
        "2026-10-05T09:00:00+00:00".parse::<PlaceTrigger>().ok(),
        None
    );
    assert_eq!("arrive:gym".parse::<PlaceTrigger>().ok(), None);
    assert_eq!(
        format!("arrive:{place}:weekly")
            .parse::<PlaceTrigger>()
            .ok(),
        None
    );
    assert_eq!(
        format!("arrive:{place}:always")
            .parse::<PlaceTrigger>()
            .ok(),
        None
    );
    assert!(PlaceTrigger::claims("arrive:gym"));
    assert!(!PlaceTrigger::claims("2026-10-05T09:00:00+00:00"));
    assert!(!PlaceTrigger::claims("0 9 * * *"));
}

#[test]
fn a_day_starts_at_local_midnight() {
    let now = "2026-10-05T23:30:00Z".parse::<DateTime<Utc>>().unwrap();
    let madrid = start_of_day(now, "Europe/Madrid");
    assert_eq!(madrid.to_rfc3339(), "2026-10-05T22:00:00+00:00");
    let utc = start_of_day(now, "UTC");
    assert_eq!(utc.to_rfc3339(), "2026-10-05T00:00:00+00:00");
    // Santiago skips from 00:00 to 01:00 on 2026-09-06.
    let now = "2026-09-06T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let santiago = start_of_day(now, "America/Santiago");
    assert_eq!(santiago.to_rfc3339(), "2026-09-06T04:00:00+00:00");
}
