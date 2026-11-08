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
    use beep_proto::names::username;
    use iroh::SecretKey;
    use secrecy::{ExposeSecret, SecretBox};
    use security_framework::{
        access_control::{ProtectionMode, SecAccessControl},
        passwords::{delete_generic_password, generic_password, set_generic_password_options},
        passwords_options::PasswordOptions,
    };
    use std::{
        fs,
        io::ErrorKind,
        path::{Path, PathBuf},
    };

    const SERVICE: &str = "com.flashmind.beep";
    const ACCOUNT: &str = "mesh-key";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    /// The saved key, unless it is unpaired and an older build paired another:
    /// then that one, so the phone stays paired across the move to this app.
    pub(super) fn load_or_create(old_file: &Path) -> Result<Identity> {
        let data = old_file.parent().context("the key file has no folder")?;
        let saved = match generic_password(PasswordOptions::new_generic_password(SERVICE, ACCOUNT))
        {
            Ok(bytes) => Some(identity(bytes)?),
            Err(e) if e.code() == NOT_FOUND => None,
            Err(e) => return Err(e).context("reading the key from the Keychain"),
        };
        let saved = match saved {
            Some(key) if paired(data, key.expose_secret()) => return Ok(key),
            saved => saved,
        };
        let from_file = match fs::read(old_file) {
            Ok(bytes) => Some(identity(bytes)?),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("reading the old key file"),
        };
        let key = if let Some(key) = from_file {
            key
        } else if let Some(native) = adopt_native(data) {
            SecretBox::new(Box::new(native))
        } else if let Some(saved) = saved {
            return Ok(saved);
        } else {
            SecretBox::new(Box::new(SecretKey::generate().to_bytes()))
        };
        save(&key)?;
        match fs::remove_file(old_file) {
            Err(e) if e.kind() != ErrorKind::NotFound => {
                return Err(e).context("removing the old key file");
            }
            _ => {}
        }
        Ok(key)
    }

    /// Whether the mesh of `key` is in `data`, where the phone opens it.
    fn paired(data: &Path, key: &[u8; 32]) -> bool {
        mesh_dir(data, key).is_dir()
    }

    fn mesh_dir(data: &Path, key: &[u8; 32]) -> PathBuf {
        data.join("workspace")
            .join(SecretKey::from_bytes(key).public().to_string())
    }

    fn save(key: &Identity) -> Result<()> {
        let mut options = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        options.set_access_control(SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleAfterFirstUnlock),
            0,
        )?);
        set_generic_password_options(key.expose_secret(), options)
            .context("saving the key in the Keychain")
    }

    pub(super) fn delete() -> Result<()> {
        match delete_generic_password(SERVICE, ACCOUNT) {
            Err(e) if e.code() != NOT_FOUND => Err(e).context("deleting the key from the Keychain"),
            _ => Ok(()),
        }
    }

    /// The native app's mesh key: Keychain item `workspace-<name>`, named
    /// after its account key in item `identity`.
    fn native_app_key() -> Option<[u8; 32]> {
        let account: [u8; 32] =
            generic_password(PasswordOptions::new_generic_password(SERVICE, "identity"))
                .ok()?
                .try_into()
                .ok()?;
        let name = username(&SecretKey::from_bytes(&account).public());
        generic_password(PasswordOptions::new_generic_password(
            SERVICE,
            &format!("workspace-{name}"),
        ))
        .ok()?
        .try_into()
        .ok()
    }

    /// The native app's key, after moving its mesh from
    /// `Application Support/workspace/<id>` into `data`. None if it never paired.
    fn adopt_native(data: &Path) -> Option<[u8; 32]> {
        let key = native_app_key()?;
        let from = data
            .parent()?
            .join("workspace")
            .join(SecretKey::from_bytes(&key).public().to_string());
        if !from.is_dir() {
            return None;
        }
        let to = mesh_dir(data, &key);
        let moved =
            fs::create_dir_all(data.join("workspace")).and_then(|()| fs::rename(&from, &to));
        match moved {
            Ok(()) => Some(key),
            Err(e) => {
                log::warn!("moving the native app's mesh: {e}");
                None
            }
        }
    }

    fn identity(bytes: Vec<u8>) -> Result<Identity> {
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("the Keychain key is not 32 bytes"))?;
        Ok(SecretBox::new(Box::new(bytes)))
    }
}
