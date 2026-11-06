//! The reports route: a desktop posts a problem report to the server, signed
//! by its device key.

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const PATH: &str = "/v1/reports";
/// The sender's endpoint id, as `EndpointId` displays it.
pub const KEY_HEADER: &str = "beep-key";
/// When it was signed, Unix milliseconds.
pub const TIME_HEADER: &str = "beep-time";
/// Hex of the 64 signature bytes over `signed_message`.
pub const SIGNATURE_HEADER: &str = "beep-signature";
pub const MAX_BODY: usize = 5 * 1024 * 1024;
/// How far the signing time may be from the server's clock.
pub const MAX_AGE: Duration = Duration::from_secs(5 * 60);

const DOMAIN: &[u8] = b"beep-report/1\0";
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// What the key signs: the domain, the body's hash and the time.
pub fn signed_message(body: &[u8], at_ms: u64) -> Vec<u8> {
    let mut message = Vec::with_capacity(DOMAIN.len() + 32 + 8);
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(blake3::hash(body).as_bytes());
    message.extend_from_slice(&at_ms.to_be_bytes());
    message
}

/// The report's name: 16 base32 characters of its hash. The same body gives
/// the same id, so a replay inside `MAX_AGE` overwrites itself.
pub fn report_id(body: &[u8]) -> String {
    let hash = blake3::hash(body);
    let mut bits = 0u32;
    let mut held = 0u32;
    let mut id = String::with_capacity(16);
    for byte in &hash.as_bytes()[..10] {
        bits = (bits << 8) | u32::from(*byte);
        held += 8;
        while held >= 5 {
            held -= 5;
            id.push(char::from(ALPHABET[((bits >> held) & 31) as usize]));
        }
        bits &= (1 << held) - 1;
    }
    id
}

/// The server's answer to an accepted report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub reference: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_message_changes_with_body_and_time() {
        let a = signed_message(b"a", 10);
        let b = signed_message(b"b", 10);
        let later = signed_message(b"a", 11);
        assert_ne!(a, b);
        assert_ne!(a, later);
        assert_ne!(b, later);
        for message in [&a, &b, &later] {
            assert!(message.starts_with(b"beep-report/1\0"));
        }
    }

    #[test]
    fn report_id_is_stable_and_short() {
        let id = report_id(b"body");
        assert_eq!(id, report_id(b"body"));
        assert_ne!(id, report_id(b"other"));
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| matches!(c, 'a'..='z' | '2'..='7')));
    }
}
