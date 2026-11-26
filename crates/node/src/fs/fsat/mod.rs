//! Folders held open (`Dir`) and file operations relative to them that never
//! follow a link, plus file metadata that differs by platform (`FileId`,
//! `Space`, `Volume`).

use anyhow::{Context, Result, bail};
use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};

// On Unix a folder is an open descriptor and every step is `*at`, so a link
// swapped in mid-way cannot redirect a write. Windows has no `*at` calls: a
// folder is its path and each step refuses reparse points along it, which
// leaves a short window between the check and the write.
mod types;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

pub(crate) use types::{FileId, Space};
pub(crate) use imp::{
    Dir, ctime_ns, id_of_file, id_of_metadata, id_of_path, is_executable, mtime_ns, open_no_follow, open_read,
    private, rename, rename_new, set_mode, volume,
};

/// Whether `error`, from opening a folder, means the path is a file or a
/// link rather than a folder.
pub(crate) fn not_a_folder(error: &io::Error) -> bool {
    imp::not_a_folder(error)
}

/// Fails unless `path` opens, without following a link, as the file
/// `identity` names.
pub(crate) fn check_same(path: &Path, identity: FileId) -> Result<()> {
    let file = open_no_follow(OpenOptions::new().read(true), path)
        .with_context(|| format!("opening {}", path.display()))?;
    let opened = id_of_metadata(&file.metadata()?);
    let named = id_of_metadata(&fs::symlink_metadata(path)?);

    if opened != identity || named != identity {
        bail!("{} changed while it was being sent", path.display());
    }

    Ok(())
}
