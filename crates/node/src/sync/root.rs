//! Knowing a project folder is the one synced before. A folder swapped for
//! another, or an empty mount point left by a disk that is not mounted,
//! would otherwise read as every file deleted, and the deletes would reach
//! every other machine.
//!
//! The folder holds a `.beep-sync` file with a random id, which sync never
//! syncs, and the state keeps that id with the folder's device and inode.

use super::rules::MARKER;
use crate::transfer::place::Folder;
use anyhow::{Context, Result, bail};
use rustix::fs::fstat;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write as _},
    os::unix::fs::MetadataExt,
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

const CHANGED: &str = "the folder changed or is not mounted";

/// Checks `root` is the folder `saved` was, given whether the state holds
/// any file seen there, and gives its identity now. Writes the marker when
/// the folder has none yet and is the same folder, or nothing was seen.
pub(super) fn check(root: &Folder, saved: Option<&RootId>, seen_any: bool) -> Result<RootId> {
    let at_path = fs::metadata(root.path())
        .with_context(|| format!("{CHANGED}: {} is missing", root.path().display()))?;
    let open = fstat(root.fd()).context("reading the open folder")?;
    let (device, inode) = (at_path.dev(), at_path.ino());
    if (open.st_dev as u64, open.st_ino as u64) != (device, inode) {
        bail!("{CHANGED}: {} is another folder now", root.path().display());
    }
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
        // The same folder whose marker went away: written again.
        (None, Some(saved)) => write_marker(root.path(), &saved.marker)?,
        (Some(marker), _) => marker,
        (None, None) => write_marker(root.path(), &new_id())?,
    };
    Ok(RootId {
        marker,
        device,
        inode,
    })
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
fn exclude_from_git(root: &Path) {
    let info = root.join(".git/info");
    if !info.is_dir() {
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
mod tests {
    use super::*;
    use crate::{files::Folders, transfer::place::open_target};
    use tempfile::TempDir;

    fn open(dir: &TempDir, name: &str) -> Folder {
        let folders = Folders::new(&[dir.path().to_owned()]);
        let path = dir.path().canonicalize().unwrap().join(name);
        open_target(&folders, &path.to_string_lossy()).unwrap()
    }

    #[test]
    fn a_folder_keeps_its_id_and_a_swapped_one_is_refused() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("game")).unwrap();
        let root = open(&dir, "game");
        let first = check(&root, None, false).unwrap();
        assert!(dir.path().join("game").join(MARKER).exists());
        assert_eq!(check(&root, Some(&first), true).unwrap(), first);

        // The marker removed by hand comes back.
        fs::remove_file(dir.path().join("game").join(MARKER)).unwrap();
        assert_eq!(check(&root, Some(&first), true).unwrap(), first);

        // Swapped for an empty folder at the same path.
        fs::rename(dir.path().join("game"), dir.path().join("old")).unwrap();
        fs::create_dir(dir.path().join("game")).unwrap();
        let error = check(&root, Some(&first), true).unwrap_err();
        assert!(format!("{error:#}").contains("not mounted"), "{error:#}");
        let reopened = open(&dir, "game");
        let error = check(&reopened, Some(&first), true).unwrap_err();
        assert!(format!("{error:#}").contains("not mounted"), "{error:#}");
        assert!(!dir.path().join("game").join(MARKER).exists());
        // With nothing seen there yet, it simply starts.
        assert!(check(&reopened, Some(&first), false).is_ok());
    }

    #[test]
    fn the_marker_stays_out_of_git_status() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("repo/.git/info")).unwrap();
        let root = open(&dir, "repo");
        check(&root, None, false).unwrap();
        let first = check(&root, None, false).unwrap();
        fs::remove_file(dir.path().join("repo").join(MARKER)).unwrap();
        check(&root, Some(&first), true).unwrap();
        let exclude = fs::read_to_string(dir.path().join("repo/.git/info/exclude")).unwrap();
        assert_eq!(exclude, format!("/{MARKER}\n"));
    }
}
