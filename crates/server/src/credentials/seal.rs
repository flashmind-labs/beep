//! Credentials are sealed before they reach the database. The key is
//! derived from the server's own secret key, which the server already keeps
//! in a file of its own, so a copy of the database alone opens nothing.

use anyhow::{Result, anyhow, ensure};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, OsRng, Payload, rand_core::RngCore},
};
use iroh::SecretKey;
use secrecy::{ExposeSecret, SecretBox, SecretString};
use std::fmt;

const KEY_CONTEXT: &str = "beep 2026-11-01 credentials at rest";
const NONCE_LEN: usize = 12;
/// The Poly1305 tag every sealed value ends with.
const TAG_LEN: usize = 16;

/// The key credentials are sealed with.
pub struct CredentialKey(SecretBox<[u8; 32]>);

impl CredentialKey {
    /// The key for the server whose identity is `server`. The same server
    /// key always gives the same credentials key.
    pub fn derive(server: &SecretKey) -> Self {
        let key = blake3::derive_key(KEY_CONTEXT, &server.to_bytes());
        Self(SecretBox::new(Box::new(key)))
    }

    /// A key nothing else knows, for a server without a key file and for
    /// tests. What it seals is lost when the process ends.
    pub fn random() -> Self {
        let mut key = [0; 32];
        OsRng.fill_bytes(&mut key);
        Self(SecretBox::new(Box::new(key)))
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(Key::from_slice(self.0.expose_secret()))
    }

    /// `plain` sealed for the credential `id`: a fresh nonce, then the
    /// ciphertext. The id is bound in, so a sealed value copied onto
    /// another row does not open.
    pub fn seal(&self, id: &str, plain: &SecretString) -> Result<Vec<u8>> {
        let mut nonce = [0; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let sealed = self
            .cipher()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plain.expose_secret().as_bytes(),
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("sealing a credential failed"))?;
        let mut out = nonce.to_vec();
        out.extend(sealed);
        Ok(out)
    }

    /// What `seal` sealed for `id`.
    pub fn open(&self, id: &str, sealed: &[u8]) -> Result<SecretString> {
        ensure!(
            sealed.len() >= NONCE_LEN + TAG_LEN,
            "a sealed credential is too short"
        );
        let (nonce, sealed) = sealed.split_at(NONCE_LEN);
        let plain = self
            .cipher()
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("credential {id} was sealed with another key"))?;
        let plain = String::from_utf8(plain).map_err(|_| anyhow!("credential {id} is not text"))?;
        Ok(SecretString::from(plain))
    }
}

impl fmt::Debug for CredentialKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialKey(hidden)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_value_opens_only_with_its_key_and_id() {
        let server = SecretKey::from_bytes(&[5; 32]);
        let key = CredentialKey::derive(&server);
        let plain = SecretString::from("ghp_example0000");
        let sealed = key.seal("c1", &plain).unwrap();
        assert!(!sealed.windows(4).any(|w| w == b"0000"));
        assert_eq!(
            key.open("c1", &sealed).unwrap().expose_secret(),
            "ghp_example0000"
        );
        // The same server key derives the same credentials key.
        let again = CredentialKey::derive(&server);
        assert!(again.open("c1", &sealed).is_ok());
        assert!(key.open("c2", &sealed).is_err());
        assert!(CredentialKey::random().open("c1", &sealed).is_err());
        assert!(key.open("c1", &sealed[..10]).is_err());
        assert!(!format!("{key:?}").contains('['));
    }
}
