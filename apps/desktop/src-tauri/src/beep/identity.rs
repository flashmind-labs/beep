//! This computer's key: in the Keychain on an iPhone, elsewhere in a file
//! only the user can read.

use anyhow::{Context, Result};
use beep_proto::link::MAX_NAME;
use iroh::SecretKey;
use secrecy::{ExposeSecret, SecretBox};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

/// The key's 32 bytes, which never print.
pub type Identity = SecretBox<[u8; 32]>;

/// Reads the key, making one on first launch. On an iPhone it is in the
/// Keychain and `path` only holds a key from a build before that.
pub fn load_or_create(path: &Path) -> Result<Identity> {
    #[cfg(target_os = "ios")]
    return keychain::load_or_create(path);
    #[cfg(not(target_os = "ios"))]
    load_or_create_file(path)
}

#[cfg_attr(target_os = "ios", allow(dead_code))]
fn load_or_create_file(path: &Path) -> Result<Identity> {
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
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
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
    #[cfg(target_os = "macos")]
    let mut name = Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Mac".into());
    #[cfg(not(target_os = "macos"))]
    let mut name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Computer".into());
    name.truncate(name.floor_char_boundary(MAX_NAME));
    name
}

/// Deletes this computer's key and state, so the next launch makes new ones.
pub fn forget(app: &AppHandle) -> Result<()> {
    let dir = data_dir(app)?;
    // The mesh was made with the old key, which is no member of a new one.
    // `mesh` is from before the compact format; it holds credentials in plain text.
    for mesh in ["mesh2", "mesh"] {
        match fs::remove_dir_all(dir.join("node").join(mesh)) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    // A phone keeps its mesh here.
    match fs::remove_dir_all(dir.join("workspace")) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    #[cfg(target_os = "ios")]
    keychain::delete()?;
    for file in [
        "identity.key",
        "state.json",
        "search.sqlite",
        "search.sqlite-wal",
        "search.sqlite-shm",
    ] {
        match fs::remove_file(dir.join(file)) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// The iPhone's key in the Keychain. It is readable after the first unlock,
/// so background work can use it, and backups and a new iPhone carry it.
#[cfg(target_os = "ios")]
mod keychain {
    use super::Identity;
    use anyhow::{Context, Result};
    use iroh::SecretKey;
    use secrecy::SecretBox;
    use security_framework::{
        access_control::{ProtectionMode, SecAccessControl},
        passwords::{delete_generic_password, generic_password, set_generic_password_options},
        passwords_options::PasswordOptions,
    };
    use std::{fs, io::ErrorKind, path::Path};

    const SERVICE: &str = "com.flashmind.beep";
    const ACCOUNT: &str = "mesh-key";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    pub(super) fn load_or_create(old_file: &Path) -> Result<Identity> {
        match generic_password(PasswordOptions::new_generic_password(SERVICE, ACCOUNT)) {
            Ok(bytes) => return identity(bytes),
            Err(e) if e.code() == NOT_FOUND => {}
            Err(e) => return Err(e).context("reading the key from the Keychain"),
        }
        // A build that kept the key in a file paired with it: keep it.
        let (key, moved) = match fs::read(old_file) {
            Ok(bytes) => (identity(bytes)?, true),
            Err(e) if e.kind() == ErrorKind::NotFound => (
                SecretBox::new(Box::new(SecretKey::generate().to_bytes())),
                false,
            ),
            Err(e) => return Err(e).context("reading the old key file"),
        };
        let mut options = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        options.set_access_control(SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleAfterFirstUnlock),
            0,
        )?);
        set_generic_password_options(secrecy::ExposeSecret::expose_secret(&key), options)
            .context("saving the key in the Keychain")?;
        if moved {
            fs::remove_file(old_file).context("removing the old key file")?;
        }
        Ok(key)
    }

    pub(super) fn delete() -> Result<()> {
        match delete_generic_password(SERVICE, ACCOUNT) {
            Err(e) if e.code() != NOT_FOUND => Err(e).context("deleting the key from the Keychain"),
            _ => Ok(()),
        }
    }

    fn identity(bytes: Vec<u8>) -> Result<Identity> {
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("the Keychain key is not 32 bytes"))?;
        Ok(SecretBox::new(Box::new(bytes)))
    }
}
