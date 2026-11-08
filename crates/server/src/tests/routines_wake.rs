use super::*;

fn at(place: &str, latitude: f64, longitude: f64) -> Location {
    Location {
        latitude,
        longitude,
        accuracy: 20.0,
        timezone: "Europe/Madrid".to_owned(),
        place: Some(place.to_owned()),
        updated_at: chrono::Utc::now(),
    }
}

#[test]
fn a_new_city_far_away_is_an_arrival() {
    let barcelona = at("Barcelona, Catalonia, Spain", 41.39, 2.16);
    let girona = at("Girona, Catalonia, Spain", 41.98, 2.82);
    let singapore = at("Singapore", 1.28, 103.85);
    let event = arrival(&barcelona, &girona).expect("a new city");
    assert_eq!(event.label, "Arrived in Girona");
    assert_eq!(event.data["new_country"], false);
    let event = arrival(&girona, &singapore).expect("a new country");
    assert_eq!(event.label, "Arrived in Singapore");
    assert_eq!(event.data["new_country"], true);
    assert!(!event.description.contains("Local time"));
}

#[test]
fn nearby_or_unknown_places_are_not_arrivals() {
    let barcelona = at("Barcelona, Catalonia, Spain", 41.39, 2.16);
    let gracia = at("Barcelona, Catalonia, Spain", 41.40, 2.15);
    let hospitalet = at("L'Hospitalet de Llobregat, Catalonia, Spain", 41.36, 2.10);
    let mut unknown = at("", 40.42, -3.70);
    unknown.place = None;
    assert!(arrival(&barcelona, &gracia).is_none());
    assert!(arrival(&barcelona, &hospitalet).is_none());
    assert!(arrival(&barcelona, &unknown).is_none());
}
