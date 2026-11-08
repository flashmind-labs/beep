use super::*;

fn requested(json: Value) -> Requested {
    serde_json::from_value(json).expect("test product is valid")
}

#[test]
fn rejects_links_that_are_not_public_pages() {
    for url in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://localhost/a",
        "https://127.0.0.1/a",
        "https://intranet/a",
        "shop.example/a",
    ] {
        let result = card(
            requested(json!({ "title": "Kettle", "url": url })),
            &mut Vec::new(),
        );
        assert!(result.is_err(), "{url}");
    }
}
