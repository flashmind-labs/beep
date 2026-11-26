//! Checks a project folder is the one synced before (`check`, `restore`), so
//! a swapped folder or an empty mount point does not read as every file
//! deleted.

use super::rules::MARKER;
use crate::{
    fs::fsat::{self, FileId},
    files::place::Folder,
};
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::{FOLDER_CHANGED as CHANGED, FOLDER_MISSING};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write as _},
    path::Path,
};

/// Which folder a project's state belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RootId {
    /// The id in its `.beep-sync` file.
    pub(super) marker: String,
    pub(super) device: u64,
    pub(super) inode: u64,
}

/// What `check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Checked {
    /// The folder synced before, or one with nothing seen yet.
    Same(RootId),
    /// The folder synced before, by device and inode, without its marker:
    /// being deleted, or only the marker removed. `rm -rf` removes the
    /// marker before the folder, so this is not trusted with deletes. Holds
    /// the saved id, for `restore` once a scan finds no file missing.
    Unmarked(RootId),
}

/// Checks `root` is the folder `saved` was, given whether the state holds
/// any file seen there, and gives its identity now. Writes the marker when
/// the folder has none yet and nothing was seen there; one that had files
/// and lost its marker is `Unmarked`.
pub(super) fn check(root: &Folder, saved: Option<&RootId>, seen_any: bool) -> Result<Checked> {
    let FileId { device, inode } = open_id(root)?;
    let same_place = saved.is_some_and(|saved| (saved.device, saved.inode) == (device, inode));
    let marker = read_marker(root.path())?;
    let marker = match (marker, saved) {
        (Some(marker), Some(saved)) if marker == saved.marker => marker,
        // Another folder's state, or none, with files seen before: not
        // this folder's to delete.
        (Some(_), Some(_)) | (None, Some(_)) if seen_any && !same_place => {
            bail!(
                "{CHANGED}: {} is not the folder synced before; nothing is synced \
                 until it is back",
                root.path().display()
            )
        }
        // The same folder whose marker went away, perhaps with its files.
        (None, Some(saved)) if seen_any => return Ok(Checked::Unmarked(saved.clone())),
        (None, Some(saved)) => write_marker(root.path(), &saved.marker)?,
        (Some(marker), _) => marker,
        (None, None) => write_marker(root.path(), &new_id())?,
    };
    Ok(Checked::Same(RootId {
        marker,
        device,
        inode,
    }))
}

/// Writes `saved`'s marker again into the folder it names.
pub(super) fn restore(root: &Folder, saved: &RootId) -> Result<()> {
    let FileId { device, inode } = open_id(root)?;

    if (device, inode) != (saved.device, saved.inode) {
        bail!("{CHANGED}: {} is another folder now", root.path().display());
    }

    write_marker(root.path(), &saved.marker)?;
    Ok(())
}

/// Which folder is at `root`'s path, which must be the one held open.
fn open_id(root: &Folder) -> Result<FileId> {
    let at_path = match fsat::id_of_path(root.path()) {
        Ok(id) => id,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            bail!("{FOLDER_MISSING}: {}", root.path().display())
        }
        Err(e) => {
            return Err(e).with_context(|| format!("{CHANGED}: {}", root.path().display()));
        }
    };
    let open = root.dir().id().context("reading the open folder")?;

    if open != at_path {
        bail!("{CHANGED}: {} is another folder now", root.path().display());
    }

    Ok(at_path)
}

fn new_id() -> String {
    format!("{:032x}", uuid::Uuid::new_v4().as_u128())
}

fn read_marker(root: &Path) -> Result<Option<String>> {
    match fs::read_to_string(root.join(MARKER)) {
        Ok(text) => Ok(Some(text.trim().to_owned()).filter(|id| !id.is_empty())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {MARKER}")),
    }
}

fn write_marker(root: &Path, id: &str) -> Result<String> {
    let path = root.join(MARKER);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .with_context(|| format!("writing {}", path.display()))?;
    writeln!(file, "{id}")?;
    file.sync_data()?;
    exclude_from_git(root);
    Ok(id.to_owned())
}

/// Keeps the marker out of `git status` in a repository.
pub(super) fn exclude_from_git(root: &Path) {
    let info = root.join(".git/info");

    if !root.join(".git").is_dir() {
        return;
    }

    if let Err(e) = fs::create_dir_all(&info) {
        log::debug!("{MARKER} is not excluded from git: {e}");
        return;
    }

    let exclude = info.join("exclude");
    let listed = fs::read_to_string(&exclude)
        .map(|text| text.lines().any(|line| line.trim() == format!("/{MARKER}")))
        .unwrap_or(false);
    if listed {
        return;
    }

    let added = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .and_then(|mut file| writeln!(file, "/{MARKER}"));
    if let Err(e) = added {
        log::debug!("{MARKER} is not excluded from git: {e}");
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_root.rs"]
mod tests;
