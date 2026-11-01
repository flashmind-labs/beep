//! Where a fetched file or folder lands: the rules for the target folder
//! and the names a source gives, the hidden partial folder it is written
//! into, and the rename that puts it in place without replacing anything.
//! Every function here touches the disk and blocks, so callers run them in
//! `spawn_blocking`.

use crate::files::Folders;
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::transfer::{ContentKind, MAX_SUFFIX, numbered};
use std::{
    fs,
    io::{self, ErrorKind},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// What a partial folder's name holds, so a sweep only removes its own.
const PARTIAL_MARK: &str = ".beep-part-";

/// `name` when it is one plain path part, as a file or folder name the
/// source gave.
pub(super) fn plain_name(name: &str) -> Result<&str> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(part)), None) if part.as_bytes() == name.as_bytes() => Ok(name),
        _ => bail!("{name:?} is not a plain file name"),
    }
}

/// `path` as a relative path of plain parts, as a folder's entries name
/// their files.
pub(super) fn relative_path(path: &str) -> Result<PathBuf> {
    ensure!(!path.is_empty(), "a folder entry has no name");
    let mut out = PathBuf::new();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(part) => out.push(part),
            _ => bail!("{path:?} is not a relative path inside the folder"),
        }
    }
    ensure!(
        out.as_os_str().len() == path.trim_end_matches('/').len(),
        "{path:?} is not a plain relative path"
    );
    Ok(out)
}

/// The folder a fetch writes into: absolute, existing, a folder, and in a
/// shared folder once its links are resolved.
pub(super) fn target_folder(folders: &Folders, folder: &str) -> Result<PathBuf> {
    ensure!(
        Path::new(folder).is_absolute(),
        "the target folder {folder:?} is not an absolute path"
    );
    let resolved = folders.resolve(folder)?;
    let metadata = resolved
        .metadata()
        .with_context(|| format!("the target folder {folder} does not exist"))?;
    ensure!(metadata.is_dir(), "{folder} is not a folder");
    Ok(resolved)
}

/// Moves `from` to `to` only if nothing is at `to`; `AlreadyExists`
/// otherwise.
pub(super) fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    match rename_exclusive(from, to) {
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) || e.kind() == ErrorKind::Unsupported => {
            // The filesystem cannot rename exclusively: check, then rename.
            // Another writer could slip in between, which only such a
            // filesystem allows.
            if to.symlink_metadata().is_ok() {
                return Err(ErrorKind::AlreadyExists.into());
            }
            fs::rename(from, to)
        }
        // A folder renamed onto a folder that is not empty.
        Err(e) if e.raw_os_error() == Some(libc::ENOTEMPTY) => Err(ErrorKind::AlreadyExists.into()),
        other => other,
    }
}

fn c_path(path: &Path) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "a path holds a NUL byte"))
}

#[cfg(target_os = "linux")]
fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (c_path(from)?, c_path(to)?);
    // SAFETY: both pointers come from CStrings that live until the call
    // returns, and AT_FDCWD is valid for any path.
    let done = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (c_path(from)?, c_path(to)?);
    // SAFETY: both pointers come from CStrings that live until the call
    // returns.
    let done = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn rename_exclusive(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(ErrorKind::Unsupported.into())
}

/// Moves `item` into `folder` as `name`, or the first free numbered name
/// after it, and gives where it landed.
pub(super) fn place(item: &Path, folder: &Path, name: &str, kind: ContentKind) -> Result<PathBuf> {
    for n in 1..=MAX_SUFFIX {
        let target = folder.join(numbered(name, kind, n));
        match rename_new(item, &target) {
            Ok(()) => return Ok(target),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("moving it to {}", target.display()));
            }
        }
    }
    bail!(
        "{name} and {MAX_SUFFIX} numbered names after it are taken in {}",
        folder.display()
    )
}

/// The partial folders being written, one file each under `dir` holding
/// the folder's path, so a node that was killed removes them next start.
#[derive(Debug, Clone)]
pub(super) struct Journal {
    dir: PathBuf,
}

impl Journal {
    pub(super) fn open(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        Ok(Self { dir })
    }

    /// Removes the partial folders a stopped node left, and their entries.
    pub(super) fn sweep(&self) -> Result<()> {
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?.path();
            let recorded = PathBuf::from(std::ffi::OsStr::from_bytes(&fs::read(&entry)?));
            let ours = recorded
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(PARTIAL_MARK));
            if ours {
                match fs::remove_dir_all(&recorded) {
                    Ok(()) => log::info!("removed a partial transfer: {}", recorded.display()),
                    Err(e) if e.kind() == ErrorKind::NotFound => {}
                    Err(e) => log::warn!("could not remove {}: {e}", recorded.display()),
                }
            }
            fs::remove_file(&entry)?;
        }
        Ok(())
    }

    /// Makes the hidden partial folder for `name` in `folder`.
    pub(super) fn start(&self, folder: &Path, name: &str) -> Result<Partial> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos());
        let id = format!(
            "{}-{}-{nanos:x}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = folder.join(format!(".{name}{PARTIAL_MARK}{id}"));
        let entry = self.dir.join(&id);
        fs::write(&entry, dir.as_os_str().as_bytes())
            .with_context(|| format!("writing {}", entry.display()))?;
        if let Err(e) = fs::create_dir(&dir) {
            // The entry would only make the next start look for nothing.
            let _ = fs::remove_file(&entry);
            return Err(e).with_context(|| format!("making {}", dir.display()));
        }
        Ok(Partial {
            dir,
            entry,
            done: false,
        })
    }
}

/// A hidden folder a fetch writes into before the rename. Removed with
/// what it holds unless `finish` was called; dropping it without `remove`
/// (the session was aborted) removes it in the background.
#[derive(Debug)]
pub(super) struct Partial {
    dir: PathBuf,
    entry: PathBuf,
    done: bool,
}

impl Partial {
    /// Where the file or folder `name` is written before it is placed.
    pub(super) fn item(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Forgets the folder once its item was moved out; it is empty.
    pub(super) fn finish(mut self) -> Result<()> {
        self.done = true;
        fs::remove_dir(&self.dir).with_context(|| format!("removing {}", self.dir.display()))?;
        fs::remove_file(&self.entry).with_context(|| format!("removing {}", self.entry.display()))
    }

    /// Removes the folder and everything written into it.
    pub(super) fn remove(mut self) -> Result<()> {
        self.done = true;
        remove_partial(&self.dir, &self.entry)
    }
}

fn remove_partial(dir: &Path, entry: &Path) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("removing {}", dir.display())),
    }
    fs::remove_file(entry).with_context(|| format!("removing {}", entry.display()))
}

impl Drop for Partial {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let (dir, entry) = (self.dir.clone(), self.entry.clone());
        let remove = move || {
            if let Err(e) = remove_partial(&dir, &entry) {
                log::warn!("a partial transfer stays until the next start: {e:#}");
            }
        };
        // Off the async workers when there are some. The journal entry is
        // removed last, so if this never runs the next start sweeps it.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(remove)),
            Err(_) => remove(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            // Only a leftover in the temp folder if this fails.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(name: &str) -> Dir {
        let dir =
            std::env::temp_dir().join(format!("beep-node-place-{name}-{}", std::process::id()));
        // A leftover from an earlier run with the same process id.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("made");
        Dir(dir.canonicalize().expect("exists"))
    }

    #[test]
    fn names_from_the_source_are_plain() {
        assert_eq!(plain_name("render.png").unwrap(), "render.png");
        for bad in ["", ".", "..", "a/b", "/abs", "a/", "../x"] {
            assert!(plain_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            relative_path("frames/0001.png").unwrap(),
            PathBuf::from("frames/0001.png")
        );
        for bad in ["", "/etc/passwd", "../out", "a/../../b", "a/./b", "a//b"] {
            assert!(relative_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_target_folder_is_absolute_existing_and_shared() {
        let dir = temp_dir("target");
        let shared = dir.0.join("shared");
        fs::create_dir_all(shared.join("in")).expect("made");
        fs::write(shared.join("file.txt"), "x").expect("written");
        let outside = dir.0.join("outside");
        fs::create_dir_all(&outside).expect("made");
        let folders = Folders::new(std::slice::from_ref(&shared));

        let found = target_folder(&folders, shared.join("in").to_str().unwrap()).unwrap();
        assert_eq!(found, shared.join("in"));
        let relative = target_folder(&folders, "in").unwrap_err();
        assert!(format!("{relative:#}").contains("not an absolute path"));
        let missing = target_folder(&folders, shared.join("nope").to_str().unwrap()).unwrap_err();
        assert!(
            format!("{missing:#}").contains("does not exist"),
            "{missing:#}"
        );
        assert!(target_folder(&folders, shared.join("file.txt").to_str().unwrap()).is_err());
        let out = target_folder(&folders, outside.to_str().unwrap()).unwrap_err();
        assert!(format!("{out:#}").contains("outside the shared folders"));
    }

    #[test]
    fn placing_keeps_both_and_never_replaces() {
        let dir = temp_dir("keep-both");
        let folder = dir.0.join("in");
        fs::create_dir_all(&folder).expect("made");
        fs::write(folder.join("render.png"), "old").expect("written");
        fs::write(folder.join("render (2).png"), "older").expect("written");

        let new = dir.0.join("new.png");
        fs::write(&new, "new").expect("written");
        let placed = place(&new, &folder, "render.png", ContentKind::File).unwrap();
        assert_eq!(placed, folder.join("render (3).png"));
        assert_eq!(
            fs::read_to_string(folder.join("render.png")).unwrap(),
            "old"
        );
        assert_eq!(fs::read_to_string(&placed).unwrap(), "new");

        // A folder never lands on a folder, empty or not, or on a file.
        fs::create_dir_all(folder.join("frames")).expect("made");
        fs::write(folder.join("frames (2)"), "a file").expect("written");
        let frames = dir.0.join("frames");
        fs::create_dir_all(&frames).expect("made");
        fs::write(frames.join("1.png"), "1").expect("written");
        let placed = place(&frames, &folder, "frames", ContentKind::Folder).unwrap();
        assert_eq!(placed, folder.join("frames (3)"));
        assert_eq!(fs::read_to_string(placed.join("1.png")).unwrap(), "1");

        let other = dir.0.join("other");
        fs::write(&other, "x").expect("written");
        let taken = rename_new(&other, &folder.join("render.png")).unwrap_err();
        assert_eq!(taken.kind(), ErrorKind::AlreadyExists);
    }

    #[test]
    fn a_stopped_nodes_partial_folders_are_swept() {
        let dir = temp_dir("sweep");
        let folder = dir.0.join("in");
        fs::create_dir_all(&folder).expect("made");
        let journal = Journal::open(dir.0.join("journal")).unwrap();
        let partial = journal.start(&folder, "big.bin").unwrap();
        fs::write(partial.item("big.bin"), "half").expect("written");
        let left = partial.dir.clone();
        // As if the node was killed: nothing removes it now.
        std::mem::forget(partial);
        assert!(left.exists());
        journal.sweep().unwrap();
        assert!(!left.exists());
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
        assert_eq!(fs::read_dir(dir.0.join("journal")).unwrap().count(), 0);

        let partial = journal.start(&folder, "a.txt").unwrap();
        fs::write(partial.item("a.txt"), "x").expect("written");
        partial.remove().unwrap();
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);
    }
}
