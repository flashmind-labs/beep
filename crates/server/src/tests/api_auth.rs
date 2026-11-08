use super::*;
use std::time::Duration;

fn signed(device: &SecretKey, server: &EndpointId, at: u64) -> SessionStart {
    let key = device.public();
    SessionStart {
        key: key.to_string(),
        at,
        signature: device
            .sign(&session_message(server, &key, at))
            .to_bytes()
            .to_vec(),
    }
}

#[test]
fn a_signed_start_buys_a_token_for_its_key_until_it_runs_out() {
    let server = SecretKey::from_bytes(&[1; 32]);
    let device = SecretKey::from_bytes(&[2; 32]);
    let tokens = Tokens::new(&server);
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let start = signed(&device, &server.public(), secs(now));
    let token = tokens.start(&start, now).expect("a token");
    let text = token.token.expose();
    assert_eq!(tokens.check(text, now), Some(device.public()));
    assert_eq!(tokens.check(text, now + SESSION_TTL), None, "ran out");
    // Another server's key does not take it.
    let other = Tokens::new(&SecretKey::from_bytes(&[3; 32]));
    assert_eq!(other.check(text, now), None);
    // Nor does a token with one byte changed.
    let mut bytes = URL_SAFE_NO_PAD.decode(text).unwrap();
    bytes[0] ^= 1;
    assert_eq!(tokens.check(&URL_SAFE_NO_PAD.encode(bytes), now), None);
}

#[test]
fn a_start_for_another_server_or_time_is_refused() {
    let server = SecretKey::from_bytes(&[1; 32]);
    let device = SecretKey::from_bytes(&[2; 32]);
    let tokens = Tokens::new(&server);
    let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let elsewhere = signed(
        &device,
        &SecretKey::from_bytes(&[3; 32]).public(),
        secs(now),
    );
    assert_eq!(
        tokens.start(&elsewhere, now),
        Err(ApiError::Unauthorized {})
    );
    let stale = signed(&device, &server.public(), secs(now) - 3600);
    assert!(matches!(
        tokens.start(&stale, now),
        Err(ApiError::BadRequest { .. })
    ));
    let mut forged = signed(&device, &server.public(), secs(now));
    forged.key = SecretKey::from_bytes(&[4; 32]).public().to_string();
    assert_eq!(tokens.start(&forged, now), Err(ApiError::Unauthorized {}));
}
