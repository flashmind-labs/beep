use super::*;
use crate::files::paths;
use crate::{files::Folders, files::place::open_target};
use std::{fs, io::Write};
use tempfile::TempDir;

fn root(dir: &TempDir) -> Folder {
    let path = paths::canonicalize(dir.path()).unwrap();
    let folders = Folders::new(std::slice::from_ref(&path));
    open_target(&folders, path.to_str().unwrap()).unwrap()
}

fn stat(path: &Path) -> Stat {
    Stat::from(&fs::metadata(path).unwrap())
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
    seal(file, mtime_ms, path)?;
    place(folder, &fd, OsStr::new("t"), path, expected)
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
