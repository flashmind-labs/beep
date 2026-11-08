use super::*;

#[test]
fn a_place_opens_in_maps_with_its_hours_and_distance() {
    let output = r#"{"action":"show_cards","cards":[
            {"kind":"place","title":"La <Pubilla>","url":"https://lapubilla.example/","site":"lapubilla.example",
             "subtitle":"Catalan","facts":["€€"],"rating":4.6,"reviews":2310,"note":"Closest",
             "place":{"address":"Plaça de la Llibertat 23","latitude":41.4,"longitude":2.15,"distance_m":648,"hours":"Open until 23:00","open_now":true}},
            {"kind":"place","title":"Bad","url":"javascript:alert(1)","site":"x"},
            "not an object"
        ],"dropped":[]}"#;
    let row = shown(output).expect("one valid card");
    assert_eq!(row.matches(r#"<a class="drec""#).count(), 1, "{row}");
    assert!(row.contains("La &lt;Pubilla&gt;"), "{row}");
    assert!(
        row.contains("https://maps.apple.com/?q=La+%3CPubilla%3E"),
        "{row}"
    );
    assert!(row.contains("&amp;ll=41.4%2C2.15"), "{row}");
    assert!(row.contains("<i>€€</i><i>650 m</i>"), "{row}");
    assert!(
        row.contains(r#"drec-status place open">Open until 23:00"#),
        "{row}"
    );
    assert!(
        row.contains("4.6 &#9733; (2,310) &middot; Closest"),
        "{row}"
    );
    assert!(!row.contains("javascript"), "{row}");
}

#[test]
fn media_and_events_add_their_parts() {
    let output = r#"{"cards":[
            {"kind":"media","title":"Aftersun","url":"https://films.example/aftersun","site":"films.example",
             "media":{"year":2022,"creator":"Charlotte Wells","watch":"Filmin"}},
            {"kind":"event","title":"Trio","url":"https://jazz.example/trio","site":"jazz.example",
             "event":{"start":"2026-10-17T21:00:00+02:00","venue":"Jamboree"}},
            {"kind":"podcast","title":"99% Invisible","url":"https://pod.example/","site":"pod.example"}
        ]}"#;
    let row = shown(output).expect("three cards");
    assert!(
        row.contains("<small>Watch on</small><b>Filmin</b>"),
        "{row}"
    );
    assert!(row.contains("2022 · Charlotte Wells"), "{row}");
    assert!(row.contains("<small>SAT</small><b>17</b>"), "{row}");
    assert!(row.contains("Sat 17 Oct, 21:00"), "{row}");
    assert!(row.contains(r#"href="https://pod.example/""#), "{row}");
    assert!(shown(r#"{"cards":[]}"#).is_none());
    assert!(shown("failed").is_none());
}

#[test]
fn distances_round_like_a_person_would() {
    assert_eq!(distance(648), "650 m");
    assert_eq!(distance(1240), "1.2 km");
    assert_eq!(distance(12_600), "13 km");
}
