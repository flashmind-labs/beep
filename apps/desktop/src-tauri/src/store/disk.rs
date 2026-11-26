//! Helpers for the files the app keeps and the state in front of them.

use anyhow::Context;
use std::{
    fs,
    path::Path,
    sync::{Mutex, MutexGuard, PoisonError},
};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Writes `bytes` to `path` through a `.part` sibling so a reader never sees
/// half of it.
pub(crate) fn write_part(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
    }

    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    fs::write(&partial, bytes).with_context(|| format!("writing {}", path.display()))?;
    fs::rename(&partial, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}
