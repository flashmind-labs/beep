//! This computer's key, kept in a file only the user can read.

use anyhow::{Context, Result};
use beep_proto::link::MAX_NAME;
use iroh::SecretKey;
use secrecy::{ExposeSecret, SecretBox};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager};

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

/// Beep's folder under Application Support, made on first use.
pub fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_data_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
    Ok(dir)
}

/// The name the phone shows for this Mac.
pub fn computer_name() -> String {
    let mut name = Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Mac".into());
    name.truncate(name.floor_char_boundary(MAX_NAME));
    name
}

/// Deletes this computer's key and state, so the next launch makes new ones.
pub fn forget(app: &AppHandle) -> Result<()> {
    let dir = data_dir(app)?;
    for file in ["identity.key", "state.json"] {
        match fs::remove_file(dir.join(file)) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
