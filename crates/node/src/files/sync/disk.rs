//! Changes files in a project folder (`look`, `place`, `remove`, `rename`),
//! relative to the open folder and without following links. Everything here
//! blocks.

use super::scan::Stat;
use crate::{
    files::fsat::{self, Dir},
    files::transfer::place::{Folder, make_dirs_at},
};
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsStr,
    fs::File,
    io,
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

/// What is at a path now.
#[derive(Debug)]
pub(super) enum Found {
    Missing,
    File(Stat),
    /// A folder, a link or a special file.
    Other,
}

impl Found {
    /// Whether it is what the last scan saw: `expected` a file, or nothing.
    fn is(&self, expected: Option<&Stat>) -> bool {
        match (self, expected) {
            (Found::Missing, None) => true,
            (Found::File(now), Some(then)) => same(now, then),
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

fn found_at(dir: &Dir, name: &OsStr) -> io::Result<Found> {
    Ok(match dir.metadata(name) {
        Ok(Some(meta)) if meta.is_file() => Found::File(Stat::of(&meta)),
        Ok(Some(_)) => Found::Other,
        Ok(None) => Found::Missing,
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => Found::Other,
        Err(e) => return Err(e),
    })
}

/// What is at `path` in the project folder.
pub(super) fn look(root: &Folder, path: &str) -> io::Result<Found> {
    let (parent, name) = split(path);
    match open_dirs(root.dir(), parent) {
        Ok(Some(dir)) => found_at(&dir, name),
        Ok(None) => Ok(Found::Missing),
        // A part of the folder is a file or a link.
        Err(e) if fsat::not_a_folder(&e) => Ok(Found::Other),
        Err(e) => Err(e),
    }
}

/// A file written into a partial folder, not yet in place.
pub(super) struct Written<'a> {
    /// The open partial folder.
    pub(super) dir: &'a Dir,
    /// Its name there.
    pub(super) name: &'a OsStr,
    pub(super) file: File,
    pub(super) mtime_ms: i64,
}

/// Finishes `written`: sets its mtime, flushes it, and moves it to `path`
/// in the project folder if what is there is still `expected`. Gives what
/// the placed file looks like.
pub(super) fn place(
    root: &Folder,
    written: Written<'_>,
    path: &str,
    expected: Option<&Stat>,
) -> Result<Stat> {
    let Written {
        dir: from,
        name: temp,
        file,
        mtime_ms,
    } = written;
    let mtime = UNIX_EPOCH + Duration::from_millis(u64::try_from(mtime_ms).unwrap_or(0));
    file.set_modified(mtime)
        .with_context(|| format!("setting the time of {path}"))?;
    file.sync_data()
        .with_context(|| format!("writing {path}"))?;
    drop(file);
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
        Found::File(stat) => Ok(stat),
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
        Found::Missing => return Ok(true),
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
        Found::File(stat) => Ok(Some(stat)),
        _ => bail!("{to} changed while it was moved"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{files::Folders, files::transfer::place::open_target};
    use std::{fs, io::Write};
    use tempfile::TempDir;

    fn root(dir: &TempDir) -> Folder {
        let path = dir.path().canonicalize().unwrap();
        let folders = Folders::new(std::slice::from_ref(&path));
        open_target(&folders, path.to_str().unwrap()).unwrap()
    }

    fn stat(path: &Path) -> Stat {
        Stat::of(&fs::metadata(path).unwrap())
    }

    fn put(
        folder: &Folder,
        dir: &TempDir,
        data: &str,
        mtime_ms: i64,
        path: &str,
        expected: Option<&Stat>,
    ) -> Result<Stat> {
        let (fd, file) = temp(dir, data);
        let written = Written {
            dir: &fd,
            name: OsStr::new("t"),
            file,
            mtime_ms,
        };
        place(folder, written, path, expected)
    }

    fn temp(dir: &TempDir, data: &str) -> (Dir, File) {
        let partial = dir.path().join(".beep-t.partial");
        fs::create_dir_all(&partial).unwrap();
        let fd = Dir::open(&partial).unwrap();
        let mut file = fd.create_file(OsStr::new("t"), false).unwrap();
        file.write_all(data.as_bytes()).unwrap();
        (fd, file)
    }

    #[test]
    fn a_file_is_replaced_only_while_it_is_what_the_scan_saw() {
        let dir = TempDir::new().unwrap();
        let folder = root(&dir);
        fs::create_dir(dir.path().join("docs")).unwrap();
        let path = dir.path().join("docs/a.txt");
        fs::write(&path, "old").unwrap();
        let seen = stat(&path);

        let placed = put(&folder, &dir, "new", 1_000, "docs/a.txt", Some(&seen)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(placed.mtime_ms(), 1_000);

        // Edited since: left alone.
        fs::write(&path, "edited here").unwrap();
        assert!(put(&folder, &dir, "newer", 2_000, "docs/a.txt", Some(&placed)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "edited here");
        // A file that appeared where none was expected is not replaced.
        assert!(put(&folder, &dir, "x", 0, "docs/a.txt", None).is_err());
        // A new file makes its folders.
        put(&folder, &dir, "deep", 0, "x/y/z.txt", None).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("x/y/z.txt")).unwrap(),
            "deep"
        );
    }

    #[test]
    fn removing_checks_the_file_and_clears_empty_folders() {
        let dir = TempDir::new().unwrap();
        let folder = root(&dir);
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        let path = dir.path().join("a/b/c.txt");
        fs::write(&path, "c").unwrap();
        let seen = stat(&path);
        fs::write(&path, "changed").unwrap();
        assert!(!remove(&folder, "a/b/c.txt", &seen).unwrap());
        assert!(path.exists());
        let seen = stat(&path);
        assert!(remove(&folder, "a/b/c.txt", &seen).unwrap());
        assert!(!dir.path().join("a").exists());
        assert!(remove(&folder, "gone.txt", &seen).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_rename_never_replaces_and_links_are_not_files() {
        let dir = TempDir::new().unwrap();
        let folder = root(&dir);
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        let seen = stat(&dir.path().join("a.txt"));
        assert!(rename(&folder, "a.txt", "b.txt", &seen).is_err());
        let moved = rename(&folder, "a.txt", "c/a (conflict 1).txt", &seen)
            .unwrap()
            .unwrap();
        assert_eq!(moved.inode, seen.inode);
        assert_eq!(
            fs::read_to_string(dir.path().join("c/a (conflict 1).txt")).unwrap(),
            "a"
        );
        std::os::unix::fs::symlink("b.txt", dir.path().join("l")).unwrap();
        assert!(matches!(look(&folder, "l").unwrap(), Found::Other));
        assert!(matches!(look(&folder, "l/x").unwrap(), Found::Other));
        assert!(matches!(look(&folder, "nope/x").unwrap(), Found::Missing));
    }
}
