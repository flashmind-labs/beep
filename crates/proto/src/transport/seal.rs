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
    aead::{Aead, Generate},
};
use curve25519_dalek::MontgomeryPoint;
use ed25519_dalek::{SigningKey, VerifyingKey};
use iroh::{EndpointId, SecretKey};

const POINT_LEN: usize = 32;
/// The Poly1305 tag every sealed notice ends with.
const TAG_LEN: usize = 16;
const KEY_CONTEXT: &str = "beep 2026-10-07 sealed push notice";

/// The encryption key for one seal. Each seal has a fresh one-off key, so
/// the nonce can stay zero.
fn cipher(
    shared: MontgomeryPoint,
    one_off: &MontgomeryPoint,
    to: &MontgomeryPoint,
) -> Result<ChaCha20Poly1305> {
    // A low-order point gives an all-zero secret anyone could compute.
    anyhow::ensure!(shared.as_bytes() != &[0; POINT_LEN], "not a usable key");
    let mut input = [0; 3 * POINT_LEN];
    input[..POINT_LEN].copy_from_slice(shared.as_bytes());
    input[POINT_LEN..2 * POINT_LEN].copy_from_slice(one_off.as_bytes());
    input[2 * POINT_LEN..].copy_from_slice(to.as_bytes());
    let key = blake3::derive_key(KEY_CONTEXT, &input);
    Ok(ChaCha20Poly1305::new(&Key::from(key)))
}

/// `notice` sealed to the phone `to`, as base64 text for a push's data field.
pub fn seal(to: &EndpointId, notice: &Notice) -> Result<String> {
    let to = VerifyingKey::from_bytes(to.as_bytes())?.to_montgomery();
    let secret = <[u8; 32]>::try_generate()?;
    let one_off = MontgomeryPoint::mul_base_clamped(secret);
    let cipher = cipher(to.mul_clamped(secret), &one_off, &to)?;
    let plain = crate::http::encode(notice)?;
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
    anyhow::ensure!(
        data.len() >= POINT_LEN + TAG_LEN,
        "a sealed notice is too short"
    );
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
#[path = "../tests/transport_seal.rs"]
mod tests;
