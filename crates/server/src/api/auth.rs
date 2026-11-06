//! Session tokens: what a device's signed `SessionStart` buys it.
//!
//! A token is `key ‖ expires ‖ mac`, unpadded base64url: the device's 32 byte
//! key, when the token runs out (seconds since the Unix epoch, eight bytes
//! big endian), and a keyed BLAKE3 hash of both under a key derived from the
//! server's own. Nothing is stored: every instance with the server's key
//! checks it, and the key is checked against its account on each request
//! anyway, so a removed device is refused at once.

use crate::protocol::{
    ApiError, SecretText,
    http::{SESSION_SKEW, SESSION_TTL, SessionStart, SessionToken, session_message},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::{EndpointId, PublicKey, SecretKey, Signature};
use std::time::{SystemTime, UNIX_EPOCH};

const KEY_CONTEXT: &str = "beep 2026-11-06 session token";
const MAC_CONTEXT: &[u8] = b"beep-token/1";
const KEY_LEN: usize = 32;
const TOKEN_LEN: usize = KEY_LEN + 8 + 32;

/// Mints and checks session tokens.
pub struct Tokens {
    server: EndpointId,
    mac_key: [u8; 32],
}

impl Tokens {
    pub fn new(server: &SecretKey) -> Self {
        Self {
            server: server.public(),
            mac_key: blake3::derive_key(KEY_CONTEXT, &server.to_bytes()),
        }
    }

    /// A token for the key `start` names, when it signed it for this
    /// server within `SESSION_SKEW` of `now`.
    pub fn start(&self, start: &SessionStart, now: SystemTime) -> Result<SessionToken, ApiError> {
        let refused = || ApiError::Unauthorized {};
        let key: PublicKey = start.key.parse().map_err(|_| refused())?;
        let signature: [u8; 64] = start
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| refused())?;
        let signature = Signature::from_bytes(&signature);
        key.verify(&session_message(&self.server, &key, start.at), &signature)
            .map_err(|_| refused())?;
        let now = secs(now);
        if start.at.abs_diff(now) > SESSION_SKEW.as_secs() {
            return Err(ApiError::bad_request(
                "this device's clock is off; set it right and try again",
            ));
        }
        let expires = now + SESSION_TTL.as_secs();
        Ok(SessionToken {
            token: SecretText::new(self.mint(&key, expires)),
            expires_in: SESSION_TTL.as_secs(),
        })
    }

    fn mint(&self, key: &EndpointId, expires: u64) -> String {
        let mut token = Vec::with_capacity(TOKEN_LEN);
        token.extend_from_slice(key.as_bytes());
        token.extend_from_slice(&expires.to_be_bytes());
        token.extend_from_slice(self.mac(key.as_bytes(), expires).as_bytes());
        URL_SAFE_NO_PAD.encode(token)
    }

    /// The key `token` was made for, when it is this server's and still good.
    pub fn check(&self, token: &str, now: SystemTime) -> Option<EndpointId> {
        let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
        if bytes.len() != TOKEN_LEN {
            return None;
        }
        let (key, rest) = bytes.split_at(KEY_LEN);
        let (expires, mac) = rest.split_at(8);
        let expires = u64::from_be_bytes(expires.try_into().ok()?);
        let mac: [u8; 32] = mac.try_into().ok()?;
        // `Hash` compares in constant time.
        if self.mac(key, expires) != blake3::Hash::from_bytes(mac) || expires <= secs(now) {
            return None;
        }
        let key: [u8; KEY_LEN] = key.try_into().ok()?;
        EndpointId::from_bytes(&key).ok()
    }

    fn mac(&self, key: &[u8], expires: u64) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new_keyed(&self.mac_key);
        hasher.update(MAC_CONTEXT);
        hasher.update(key);
        hasher.update(&expires.to_be_bytes());
        hasher.finalize()
    }
}

fn secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

#[cfg(test)]
mod tests {
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
}
