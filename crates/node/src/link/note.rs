//! Small files the node keeps in its folder: written whole or not at all,
//! and read back as nothing, with a warning, when they cannot be.

use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    ffi::OsString,
    fs,
    io::{ErrorKind, Write as _},
    path::{Path, PathBuf},
};

/// The JSON at `path`, or the default when there is none or it cannot be
/// read.
pub(crate) fn read<T: DeserializeOwned + Default>(path: &Path) -> T {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return T::default(),
        Err(e) => {
            log::warn!("could not read {}: {e}", path.display());
            return T::default();
        }
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        log::warn!("ignored {}: {e}", path.display());
        T::default()
    })
}

/// Writes `value` to `path` as JSON.
pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write(path, &serde_json::to_vec(value)?)
}

/// Writes `bytes` to `path` through a temporary file beside it, renamed
/// over it once on disk.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = OsString::from(path);
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let mut out = fs::File::create(&temp).with_context(|| format!("writing {}", temp.display()))?;
    out.write_all(bytes)?;
    out.sync_data()?;
    fs::rename(&temp, path).with_context(|| format!("writing {}", path.display()))
}
