//! Changes files in a project folder (`look`, `place`, `remove`, `rename`),
//! relative to the open folder and without following links. Everything here
//! blocks.

use crate::{
    files::place::{Folder, make_dirs_at},
    fs::fsat::{self, Dir, FileId},
};
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsStr,
    fs::{File, Metadata},
    io,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

/// What a file looked like on disk, to see whether it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Stat {
    pub(super) size: u64,
    pub(super) mtime_ns: i64,
    pub(super) ctime_ns: i64,
    pub(super) inode: u64,
    pub(super) device: u64,
    pub(super) executable: bool,
}

impl From<&Metadata> for Stat {
    fn from(meta: &Metadata) -> Self {
        let id = fsat::id_of_metadata(meta);
        Self {
            size: meta.len(),
            mtime_ns: fsat::mtime_ns(meta),
            ctime_ns: fsat::ctime_ns(meta),
            inode: id.inode,
            device: id.device,
            executable: fsat::is_executable(meta),
        }
    }
}

impl Stat {
    pub(super) fn mtime_ms(&self) -> i64 {
        self.mtime_ns.div_euclid(1_000_000)
    }

    pub(super) fn id(&self) -> FileId {
        FileId {
            device: self.device,
            inode: self.inode,
        }
    }
}

/// A file sync keeps.
#[derive(Debug, Clone)]
pub(super) struct Found {
    /// Relative to the project folder, parts joined with `/`.
    pub(super) path: String,
    pub(super) abs: PathBuf,
    pub(super) stat: Stat,
}

/// What is at a path now.
#[derive(Debug)]
pub(super) enum OnDisk {
    Missing,
    File(Stat),
    /// A folder, a link or a special file.
    Other,
}

impl OnDisk {
    /// Whether it is what the last scan saw: `expected` a file, or nothing.
    fn is(&self, expected: Option<&Stat>) -> bool {
        match (self, expected) {
            (OnDisk::Missing, None) => true,
            (OnDisk::File(now), Some(then)) => same(now, then),
            _ => false,
        }
    }
}

/// Whether a file is unchanged; the device is left out, since it is the
/// same folder. A file is replaced or removed only while it is still what
/// the last scan saw, so an edit made since is never lost: the next scan
/// picks it up.
pub(super) fn same(now: &Stat, then: &Stat) -> bool {
    now.size == then.size
        && now.mtime_ns == then.mtime_ns
        && now.ctime_ns == then.ctime_ns
        && now.inode == then.inode
}

/// Splits `path` into its folder parts and its name.
fn split(path: &str) -> (&Path, &OsStr) {
    match path.rfind('/') {
        Some(at) => (Path::new(&path[..at]), OsStr::new(&path[at + 1..])),
        None => (Path::new(""), OsStr::new(path)),
    }
}

/// The folder `path` names under `dir`, if it is there.
fn open_dirs(dir: &Dir, path: &Path) -> io::Result<Option<Dir>> {
    let mut current = dir.try_clone()?;

    for part in path.components() {
        current = match current.open_dir(part.as_os_str()) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
    }

    Ok(Some(current))
}

fn found_at(dir: &Dir, name: &OsStr) -> io::Result<OnDisk> {
    Ok(match dir.metadata(name) {
        Ok(Some(meta)) if meta.is_file() => OnDisk::File(Stat::from(&meta)),
        Ok(Some(_)) => OnDisk::Other,
        Ok(None) => OnDisk::Missing,
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => OnDisk::Other,
        Err(e) => return Err(e),
    })
}

/// What is at `path` in the project folder.
pub(super) fn look(root: &Folder, path: &str) -> io::Result<OnDisk> {
    let (parent, name) = split(path);

    match open_dirs(root.dir(), parent) {
        Ok(Some(dir)) => found_at(&dir, name),
        Ok(None) => Ok(OnDisk::Missing),
        // A part of the folder is a file or a link.
        Err(e) if fsat::not_a_folder(&e) => Ok(OnDisk::Other),
        Err(e) => Err(e),
    }
}

/// Finishes the file written into a partial folder and closes it. Only where
/// `Dir::sync_disk` does nothing is it flushed here, on its own.
pub(super) fn seal(file: File, mtime_ms: i64, path: &str) -> Result<()> {
    let mtime = UNIX_EPOCH + Duration::from_millis(u64::try_from(mtime_ms).unwrap_or(0));
    file.set_modified(mtime)
        .with_context(|| format!("setting the time of {path}"))?;
    if !cfg!(target_os = "linux") {
        file.sync_data()
            .with_context(|| format!("writing {path}"))?;
    }

    Ok(())
}

/// Moves `temp`, sealed and on disk, from the partial folder `from` to `path`
/// in the project folder if what is there is still `expected`. It must reach
/// the disk first: a crash after the rename could otherwise leave a file that
/// looks like an edit at `path` and holds nothing. Gives what the placed file
/// looks like.
pub(super) fn place(
    root: &Folder,
    from: &Dir,
    temp: &OsStr,
    path: &str,
    expected: Option<&Stat>,
) -> Result<Stat> {
    let (parent, name) = split(path);
    let dir =
        make_dirs_at(root.dir(), parent).with_context(|| format!("making the folder of {path}"))?;
    let now = found_at(&dir, name)?;

    if !now.is(expected) {
        // Ours to remove; the file there is left alone.
        let _ = from.remove_file(temp);
        bail!("{path} changed on this machine; it is merged on the next scan");
    }

    let moved = match expected {
        None => fsat::rename_new(from, temp, &dir, name, false),
        Some(_) => fsat::rename(from, temp, &dir, name),
    };

    if let Err(e) = moved {
        let _ = from.remove_file(temp);
        return Err(e).with_context(|| format!("moving {path} into place"));
    }

    match found_at(&dir, name)? {
        OnDisk::File(stat) => Ok(stat),
        _ => bail!("{path} changed while it was put in place"),
    }
}

/// Removes `path` if it is still `expected`, and the folders it leaves
/// empty. `false` when it changed and was left.
pub(super) fn remove(root: &Folder, path: &str, expected: &Stat) -> Result<bool> {
    let (parent, name) = split(path);
    let Some(dir) = open_dirs(root.dir(), parent)? else {
        return Ok(true);
    };

    match found_at(&dir, name)? {
        OnDisk::Missing => return Ok(true),
        found if !found.is(Some(expected)) => return Ok(false),
        _ => {}
    }

    dir.remove_file(name)
        .with_context(|| format!("removing {path}"))?;
    remove_empty(root, parent);
    Ok(true)
}

/// Removes `folder` and the folders above it while they are empty.
fn remove_empty(root: &Folder, mut folder: &Path) {
    while let (Some(parent), Some(name)) = (folder.parent(), folder.file_name()) {
        let Ok(Some(dir)) = open_dirs(root.dir(), parent) else {
            return;
        };

        if dir.remove_dir(name).is_err() {
            return;
        }
        folder = parent;
    }
}

/// Moves `from` to `to`, a free path, if `from` is still `expected`; gives
/// what the moved file looks like, or `None` when it changed.
pub(super) fn rename(root: &Folder, from: &str, to: &str, expected: &Stat) -> Result<Option<Stat>> {
    let (from_parent, from_name) = split(from);
    let Some(from_dir) = open_dirs(root.dir(), from_parent)? else {
        return Ok(None);
    };

    if !found_at(&from_dir, from_name)?.is(Some(expected)) {
        return Ok(None);
    }

    let (to_parent, to_name) = split(to);
    let to_dir = make_dirs_at(root.dir(), to_parent)
        .with_context(|| format!("making the folder of {to}"))?;
    fsat::rename_new(&from_dir, from_name, &to_dir, to_name, false)
        .with_context(|| format!("moving {from} to {to}"))?;
    match found_at(&to_dir, to_name)? {
        OnDisk::File(stat) => Ok(Some(stat)),
        _ => bail!("{to} changed while it was moved"),
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_disk.rs"]
mod tests;
