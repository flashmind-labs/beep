//! Notices sent through Firebase are sealed to the phone's endpoint id, so
//! Google carries them without reading them. The ed25519 endpoint key
//! doubles as an X25519 key: each seal makes a one-off key pair, agrees a
//! secret with the phone's key, and encrypts with a key derived from it.
//! Only the phone's secret key opens it.

use crate::Notice;
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use curve25519_dalek::MontgomeryPoint;
use ed25519_dalek::{SigningKey, VerifyingKey};
use iroh::{EndpointId, SecretKey};

const POINT_LEN: usize = 32;
const KEY_CONTEXT: &str = "beep 2026-10-07 sealed push notice";

/// The encryption key for one seal. Each seal has a fresh one-off key, so
/// the nonce can stay zero.
fn cipher(
    shared: MontgomeryPoint,
    one_off: &MontgomeryPoint,
    to: &MontgomeryPoint,
) -> Result<ChaCha20Poly1305> {
    // A low-order point gives an all-zero secret anyone could compute.
    anyhow::ensure!(shared.as_bytes() != &[0; 32], "not a usable key");
    let mut input = [0; 3 * POINT_LEN];
    input[..32].copy_from_slice(shared.as_bytes());
    input[32..64].copy_from_slice(one_off.as_bytes());
    input[64..].copy_from_slice(to.as_bytes());
    let key = blake3::derive_key(KEY_CONTEXT, &input);
    Ok(ChaCha20Poly1305::new(Key::from_slice(&key)))
}

/// `notice` sealed to the phone `to`, as base64 text for a push's data field.
pub fn seal(to: &EndpointId, notice: &Notice) -> Result<String> {
    let to = VerifyingKey::from_bytes(to.as_bytes())?.to_montgomery();
    let mut secret = [0; 32];
    OsRng.fill_bytes(&mut secret);
    let one_off = MontgomeryPoint::mul_base_clamped(secret);
    let cipher = cipher(to.mul_clamped(secret), &one_off, &to)?;
    let plain = rmp_serde::to_vec(notice)?;
    let sealed = cipher
        .encrypt(&Nonce::default(), plain.as_slice())
        .map_err(|_| anyhow::anyhow!("sealing a notice failed"))?;
    let mut out = one_off.to_bytes().to_vec();
    out.extend(sealed);
    Ok(STANDARD.encode(out))
}

/// The notice `seal` sealed to the phone whose secret key is `identity`.
pub fn open(identity: &SecretKey, sealed: &str) -> Result<Notice> {
    let data = STANDARD
        .decode(sealed)
        .context("a sealed notice is base64")?;
    anyhow::ensure!(data.len() > POINT_LEN, "a sealed notice is too short");
    let (one_off, sealed) = data.split_at(POINT_LEN);
    let one_off = MontgomeryPoint(one_off.try_into()?);
    let signing = SigningKey::from_bytes(&identity.to_bytes());
    let to = signing.verifying_key().to_montgomery();
    let cipher = cipher(
        one_off.mul_clamped(signing.to_scalar_bytes()),
        &one_off,
        &to,
    )?;
    let plain = cipher
        .decrypt(&Nonce::default(), sealed)
        .map_err(|_| anyhow::anyhow!("the notice was sealed to another phone"))?;
    Ok(rmp_serde::from_slice(&plain)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice() -> Notice {
        Notice {
            title: "Done".into(),
            body: "The report is ready.".into(),
            session_id: "s1".into(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        }
    }

    #[test]
    fn a_sealed_notice_opens_on_its_phone_only() {
        let phone = SecretKey::from_bytes(&[1; 32]);
        let other = SecretKey::from_bytes(&[2; 32]);
        let sealed = seal(&phone.public(), &notice()).unwrap();
        assert_eq!(open(&phone, &sealed).unwrap(), notice());
        assert!(open(&other, &sealed).is_err());
        // A fresh one-off key each time.
        assert_ne!(seal(&phone.public(), &notice()).unwrap(), sealed);
    }
}
