//! Seals credentials with a key derived from the server's secret key, so the database alone opens nothing.

use anyhow::{Result, anyhow, ensure};
use beep_projects::credentials::Slot;
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, Generate, Payload},
};
use iroh::SecretKey;
use secrecy::{ExposeSecret, SecretBox, SecretString};
use std::fmt;

const KEY_CONTEXT: &str = "beep 2026-11-01 credentials at rest";
const NONCE_LEN: usize = 12;
/// The Poly1305 tag every sealed value ends with.
const TAG_LEN: usize = 16;

/// Associated data for a sealed value, so it does not open on another row, user or slot.
pub struct Owner<'a> {
    /// The user's key in the store.
    pub user: &'a str,
    pub slot: &'a Slot,
    /// The credential's row id.
    pub id: &'a str,
}

impl Owner<'_> {
    fn aad(&self) -> Vec<u8> {
        let slot = match self.slot {
            Slot::GitHub => "github".to_owned(),
            Slot::Forgejo(url) => format!("forgejo {url}"),
            Slot::ApiKey(provider) => format!("api_key {}", provider.as_ref()),
            Slot::AgentToken(agent) => format!("agent_token {}", agent.as_ref()),
        };
        // NUL never occurs in any of them, so the parts cannot run together.
        [self.user, &slot, self.id].join("\0").into_bytes()
    }
}

/// Associated data for a harness's env vars: user, machine and agent.
pub struct HarnessOwner<'a> {
    pub user: &'a str,
    pub machine: &'a str,
    pub agent: &'a str,
}

impl HarnessOwner<'_> {
    fn aad(&self) -> Vec<u8> {
        ["harness_env", self.user, self.machine, self.agent]
            .join("\0")
            .into_bytes()
    }
}

pub struct CredentialKey(SecretBox<[u8; 32]>);

impl CredentialKey {
    /// The key for the server whose identity is `server`, deterministically.
    pub fn derive(server: &SecretKey) -> Self {
        let key = blake3::derive_key(KEY_CONTEXT, &server.to_bytes());
        Self(SecretBox::new(Box::new(key)))
    }

    /// A throwaway key; what it seals is lost when the process ends.
    pub fn random() -> Self {
        Self(SecretBox::new(Box::new(<[u8; 32]>::generate())))
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(&Key::from(*self.0.expose_secret()))
    }

    /// `plain` sealed for `owner`: a fresh nonce, then the ciphertext.
    pub fn seal(&self, owner: &Owner<'_>, plain: &SecretString) -> Result<Vec<u8>> {
        self.seal_for(&owner.aad(), plain)
    }

    /// `plain` sealed for a harness's env vars.
    pub fn seal_harness(&self, owner: &HarnessOwner<'_>, plain: &SecretString) -> Result<Vec<u8>> {
        self.seal_for(&owner.aad(), plain)
    }

    /// What `seal_harness` sealed for `owner`.
    pub fn open_harness(&self, owner: &HarnessOwner<'_>, sealed: &[u8]) -> Result<SecretString> {
        let what = format!("the {} settings", owner.agent);
        self.open_for(&owner.aad(), sealed, &what)
    }

    fn seal_for(&self, aad: &[u8], plain: &SecretString) -> Result<Vec<u8>> {
        let nonce = <[u8; NONCE_LEN]>::try_generate()?;
        let sealed = self
            .cipher()
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: plain.expose_secret().as_bytes(),
                    aad,
                },
            )
            .map_err(|_| anyhow!("sealing a credential failed"))?;
        let mut out = nonce.to_vec();
        out.extend(sealed);
        Ok(out)
    }

    /// What `seal` sealed for `owner`.
    pub fn open(&self, owner: &Owner<'_>, sealed: &[u8]) -> Result<SecretString> {
        self.open_for(&owner.aad(), sealed, &format!("credential {}", owner.id))
    }

    fn open_for(&self, aad: &[u8], sealed: &[u8], id: &str) -> Result<SecretString> {
        ensure!(
            sealed.len() >= NONCE_LEN + TAG_LEN,
            "a sealed credential is too short"
        );
        let (nonce, sealed) = sealed.split_at(NONCE_LEN);
        let plain = self
            .cipher()
            .decrypt(&Nonce::try_from(nonce)?, Payload { msg: sealed, aad })
            .map_err(|_| anyhow!("{id} was sealed with another key or for another owner"))?;
        let plain = String::from_utf8(plain).map_err(|_| anyhow!("{id} is not text"))?;
        Ok(SecretString::from(plain))
    }
}

impl fmt::Debug for CredentialKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialKey(hidden)")
    }
}

#[cfg(test)]
#[path = "../../tests/account_credentials_seal.rs"]
mod tests;
