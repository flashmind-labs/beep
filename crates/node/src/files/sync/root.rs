//! Checks a project folder is the one synced before (`check`, `restore`), so
//! a swapped folder or an empty mount point does not read as every file
//! deleted.

use super::rules::MARKER;
use crate::{
    files::fsat::{self, FileId},
    files::transfer::place::Folder,
};
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::FOLDER_MISSING;
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

const CHANGED: &str = "the folder changed or is not mounted";

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
mod tests {
    use super::*;
    use crate::files::paths;
    use crate::{files::Folders, files::transfer::place::open_target};
    use tempfile::TempDir;

    fn open(dir: &TempDir, name: &str) -> Folder {
        let folders = Folders::new(&[dir.path().to_owned()]);
        let path = paths::canonicalize(dir.path()).unwrap().join(name);
        open_target(&folders, &path.to_string_lossy()).unwrap()
    }

    #[test]
    fn a_folder_keeps_its_id_and_a_swapped_one_is_refused() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("game")).unwrap();
        let root = open(&dir, "game");
        let Checked::Same(first) = check(&root, None, false).unwrap() else {
            panic!("a new folder is marked");
        };
        assert!(dir.path().join("game").join(MARKER).exists());
        let same = Checked::Same(first.clone());
        assert_eq!(check(&root, Some(&first), true).unwrap(), same);

        // The marker removed: not trusted, nor written, until restored.
        fs::remove_file(dir.path().join("game").join(MARKER)).unwrap();
        assert_eq!(
            check(&root, Some(&first), true).unwrap(),
            Checked::Unmarked(first.clone())
        );
        assert!(!dir.path().join("game").join(MARKER).exists());
        restore(&root, &first).unwrap();
        assert_eq!(check(&root, Some(&first), true).unwrap(), same);

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
        let Checked::Same(first) = check(&root, None, false).unwrap() else {
            panic!("a new folder is marked");
        };
        fs::remove_file(dir.path().join("repo").join(MARKER)).unwrap();
        restore(&root, &first).unwrap();
        let exclude = fs::read_to_string(dir.path().join("repo/.git/info/exclude")).unwrap();
        assert_eq!(exclude, format!("/{MARKER}\n"));
    }
}
