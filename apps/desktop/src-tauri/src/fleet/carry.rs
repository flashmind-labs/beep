//! The settings a Mac brings when it leaves its own mesh for another. They
//! wait in one file, written before the swap and removed once every one is
//! set in the new mesh, so a crash between the two loses none: the next
//! start sets them.

use crate::error::{DesktopError, Result};
use beep_mesh::{Key, Mesh};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    str::FromStr,
};

fn file(dir: &Path) -> PathBuf {
    dir.join("carry.json")
}

/// Writes the settings to disk, readable by this user only.
pub(super) fn save(dir: &Path, settings: &[(Key, Vec<u8>)]) -> Result<()> {
    let texts: Vec<(String, &[u8])> = settings
        .iter()
        .map(|(key, value)| (key.to_string(), value.as_slice()))
        .collect();
    let bytes = serde_json::to_vec(&texts).map_err(anyhow::Error::from)?;
    let write = || -> std::io::Result<()> {
        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(file(dir))?;
        out.write_all(&bytes)?;
        out.sync_all()
    };
    write().map_err(|e| anyhow::Error::from(e).into())
}

/// Forgets the file.
pub(super) fn discard(dir: &Path) {
    match fs::remove_file(file(dir)) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            log::warn!("could not remove the settings a join carried: {e}");
        }
        _ => {}
    }
}

/// Sets what the file holds in `mesh`, if there is a file, for the keys the mesh never held: a
/// value the mesh has, or deleted, is newer than what this Mac carried. Every setting is
/// tried. The file goes once all are set; otherwise it stays for the next
/// start and the error names each one that failed.
pub(super) async fn replay(dir: &Path, mesh: &Mesh) -> Result<()> {
    let bytes = match fs::read(file(dir)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(anyhow::Error::from(e).into()),
    };
    let settings: Vec<(String, Vec<u8>)> =
        serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
    let mut failed = Vec::new();
    for (text, value) in settings {
        let set = match Key::from_str(&text) {
            Ok(key) => mesh.set_if_new(key, value).await.map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = set {
            failed.push(format!("{text}: {e}"));
        }
    }
    if failed.is_empty() {
        discard(dir);
        return Ok(());
    }
    Err(DesktopError::BadRequest(format!(
        "settings not carried over: {}",
        failed.join("; ")
    )))
}
