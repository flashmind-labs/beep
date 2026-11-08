use super::*;
use axum::http::HeaderValue;
use beep_report::{Period, Sender, draft, zip};
use iroh::SecretKey;

const NOW_MS: u64 = 1_800_000_000_000;

fn report(key: &SecretKey) -> Vec<u8> {
    let sender = Sender {
        id: key.public(),
        mesh: None,
        app: "desktop",
        app_version: "0.2.0".into(),
    };
    let draft = draft("it broke", Period::Today, 0, sender, Vec::new()).expect("a draft");
    zip(&draft).expect("a zip")
}

fn signed(key: &SecretKey, body: &[u8], at_ms: u64) -> HeaderMap {
    let signature = key.sign(&signed_message(body, at_ms));
    let mut headers = HeaderMap::new();
    for (name, value) in [
        (KEY_HEADER, key.public().to_string()),
        (TIME_HEADER, at_ms.to_string()),
        (SIGNATURE_HEADER, hex::encode(signature.to_bytes())),
    ] {
        let value = HeaderValue::from_str(&value).expect("ASCII header values");
        headers.insert(name, value);
    }
    headers
}

#[test]
fn a_signed_report_passes() {
    let key = SecretKey::generate();
    let body = report(&key);
    let manifest = check(&signed(&key, &body, NOW_MS), &body, NOW_MS).expect("it passes");
    assert_eq!(manifest.sender, key.public());
}

#[test]
fn a_report_from_the_future_is_refused_too() {
    let key = SecretKey::generate();
    let body = report(&key);
    let ahead = NOW_MS + 6 * 60_000;
    let refused = check(&signed(&key, &body, ahead), &body, NOW_MS).map(|m| m.sender);
    assert_eq!(refused, Err(Refused::Clock(6)));
    assert_eq!(
        Refused::Clock(6).to_string(),
        "This computer's clock is off by 6 minutes; set it right and send again."
    );
}

#[test]
fn a_missing_header_is_named() {
    let key = SecretKey::generate();
    let body = report(&key);
    let mut headers = signed(&key, &body, NOW_MS);
    headers.remove(TIME_HEADER);
    let refused = check(&headers, &body, NOW_MS).map(|m| m.sender);
    assert_eq!(refused, Err(Refused::Header(TIME_HEADER)));
}
