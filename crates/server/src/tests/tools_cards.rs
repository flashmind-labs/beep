use super::*;

fn requested(json: Value) -> Requested {
    serde_json::from_value(json).expect("test card is valid")
}

fn card_of(json: Value) -> (Card, Vec<String>) {
    let mut dropped = Vec::new();
    let card = card(requested(json), &mut dropped).expect("a valid card");
    (card, dropped)
}

#[test]
fn rejects_bad_kinds_products_and_links() {
    for (kind, url) in [
        ("two words", "https://a.example/"),
        ("", "https://a.example/"),
        ("product", "https://a.example/"),
        ("place", "javascript:alert(1)"),
        ("place", "https://localhost/a"),
    ] {
        let result = card(
            requested(json!({ "kind": kind, "title": "Thing", "url": url })),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "{kind} {url}");
    }
}

#[test]
fn drops_bad_place_details() {
    let (card, dropped) = card_of(json!({
        "kind": "place", "title": "Bar", "url": "https://bar.example/",
        "place": { "latitude": 41.4, "phone": "call us", "booking_url": "file:///etc/passwd" }
    }));
    assert_eq!(card.place.expect("a place part"), PlacePart::default());
    assert_eq!(dropped.len(), 3, "{dropped:?}");
}
