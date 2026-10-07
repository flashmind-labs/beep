//! This computer's key, kept in a file only the user can read.

use anyhow::{Context, Result};
use iroh::SecretKey;
use secrecy::{ExposeSecret, SecretBox};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

/// The key's 32 bytes, which never print.
pub type Identity = SecretBox<[u8; 32]>;

/// Reads the key at `path`, making one on first launch.
pub fn load_or_create(path: &Path) -> Result<Identity> {
    if let Ok(bytes) = fs::read(path) {
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("{} is not a 32 byte key", path.display()))?;
        return Ok(SecretBox::new(Box::new(bytes)));
    }
    let key = SecretKey::generate().to_bytes();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    file.write_all(&key)?;
    Ok(SecretBox::new(Box::new(key)))
}

pub fn secret_key(identity: &Identity) -> SecretKey {
    SecretKey::from_bytes(identity.expose_secret())
}
