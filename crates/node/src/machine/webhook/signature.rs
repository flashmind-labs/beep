//! Checks a webhook delivery's HMAC-SHA256 signature against the watch's
//! secret (`verify`).

use hmac::{Hmac, KeyInit, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

/// The headers a signature may come in, lowercased, and the prefix each
/// value has. GitHub sends `sha256=<hex>`; Forgejo and Gitea send bare
/// hex, and newer ones send `X-Hub-Signature-256` too.
const SIGNATURES: [(&str, &str); 3] = [
    ("x-hub-signature-256", "sha256="),
    ("x-forgejo-signature", ""),
    ("x-gitea-signature", ""),
];

/// Whether one of `headers` (lowercased names) signs `body` with `secret`.
/// No signature at all fails.
pub(super) fn verify(secret: &SecretString, headers: &[(String, String)], body: &[u8]) -> bool {
    headers.iter().any(|(name, value)| {
        SIGNATURES
            .iter()
            .find(|(header, _)| header == name)
            .and_then(|(_, prefix)| value.trim().strip_prefix(prefix))
            .and_then(|hex| hex::decode(hex).ok())
            .is_some_and(|signature| matches(secret, body, &signature))
    })
}

/// Compares in constant time.
fn matches(secret: &SecretString, body: &[u8], signature: &[u8]) -> bool {
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.expose_secret().as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(signature).is_ok()
}

#[cfg(test)]
pub(super) fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("any key length works");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> SecretString {
        SecretString::from("hook-secret")
    }

    fn header(name: &str, value: String) -> Vec<(String, String)> {
        vec![(name.to_owned(), value)]
    }

    #[test]
    fn a_github_signature_checks_out() {
        let body = br#"{"action":"opened"}"#;
        let value = format!("sha256={}", sign("hook-secret", body));
        assert!(verify(
            &secret(),
            &header("x-hub-signature-256", value),
            body
        ));
    }

    #[test]
    fn a_forgejo_signature_checks_out() {
        let body = br#"{"ref":"refs/heads/main"}"#;
        for name in ["x-forgejo-signature", "x-gitea-signature"] {
            assert!(verify(
                &secret(),
                &header(name, sign("hook-secret", body)),
                body
            ));
        }
    }

    #[test]
    fn a_bad_signature_is_rejected() {
        let body = br#"{"action":"opened"}"#;
        let wrong_key = format!("sha256={}", sign("someone-else", body));
        assert!(!verify(
            &secret(),
            &header("x-hub-signature-256", wrong_key),
            body
        ));
        let changed_body = format!("sha256={}", sign("hook-secret", b"{}"));
        assert!(!verify(
            &secret(),
            &header("x-hub-signature-256", changed_body),
            body
        ));
        // GitHub's header without its prefix, junk, or nothing at all.
        let bare = sign("hook-secret", body);
        assert!(!verify(
            &secret(),
            &header("x-hub-signature-256", bare),
            body
        ));
        assert!(!verify(
            &secret(),
            &header("x-gitea-signature", "zz".into()),
            body
        ));
        assert!(!verify(&secret(), &[], body));
        // A signature in a header nobody signs with does not count.
        let elsewhere = sign("hook-secret", body);
        assert!(!verify(&secret(), &header("x-signature", elsewhere), body));
    }
}
