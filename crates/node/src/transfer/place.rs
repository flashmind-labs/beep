//! Where a fetched file or folder lands. The target folder is opened once,
//! by walking down from its shared folder without following links, and
//! every later step (the hidden partial folder, each file and folder in it,
//! the final rename) happens relative to that open folder, so a link
//! swapped in during a long download cannot send writes elsewhere.
//! Everything here blocks: callers run it in `spawn_blocking`.

use crate::{
    files::Folders,
    fsat::{self, Dir},
};
use anyhow::{Context, Result, bail, ensure};
use beep_proto::node::transfer::{ContentKind, MAX_SUFFIX, numbered};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, File},
    io::{self, ErrorKind},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// How a partial folder's name starts and ends, so a sweep only removes
/// its own and sync tools that skip dot files and `*.partial` skip it.
const PARTIAL_START: &str = ".beep-";
const PARTIAL_END: &str = ".partial";

pub(crate) use fsat::Space;

/// `name` when it is one plain path part, as a file or folder name the
/// source gave.
pub(crate) fn plain_name(name: &str) -> Result<&str> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(part)), None) if part == OsStr::new(name) => Ok(name),
        _ => bail!("{name:?} is not a plain file name"),
    }
}

/// `path` as a relative path of plain parts, as a folder's entries name
/// their files.
pub(crate) fn relative_path(path: &str) -> Result<PathBuf> {
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

/// A target folder, held open.
#[derive(Debug)]
pub(crate) struct Folder {
    dir: Dir,
    /// Its path when it was opened, for messages and the result.
    path: PathBuf,
}

impl Folder {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The open folder, which writes are made relative to.
    pub(crate) fn dir(&self) -> &Dir {
        &self.dir
    }

    /// Bytes free on its disk, and the disk's id.
    pub(crate) fn space(&self) -> Result<Space> {
        self.dir
            .space()
            .with_context(|| format!("reading free space at {}", self.path.display()))
    }
}

/// Free bytes on the disk that holds `dir`.
pub(crate) fn space_at(dir: &Path) -> Result<Space> {
    let opened = Dir::open(dir).with_context(|| format!("opening {}", dir.display()))?;
    opened
        .space()
        .with_context(|| format!("reading free space at {}", dir.display()))
}

/// Opens the folder a fetch writes into: absolute, existing, a folder, and
/// reached from its shared folder through plain folders only.
pub(crate) fn open_target(folders: &Folders, folder: &str) -> Result<Folder> {
    ensure!(
        Path::new(folder).is_absolute(),
        "the target folder {folder:?} is not an absolute path"
    );
    let resolved = folders.resolve(folder)?;
    let root = folders
        .root_of(&resolved)
        .with_context(|| format!("{folder} is outside the shared folders"))?;
    let mut dir = Dir::open(root).with_context(|| format!("opening {}", root.display()))?;
    let rest = resolved.strip_prefix(root)?;
    for part in rest.components() {
        dir = match dir.open_dir(part.as_os_str()) {
            Ok(dir) => dir,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                bail!("the target folder {folder} does not exist")
            }
            Err(e) => return Err(e).with_context(|| format!("{folder} is not a folder")),
        };
    }
    Ok(Folder {
        dir,
        path: resolved,
    })
}

/// The folder `name` in `dir`, made if missing, opened without following
/// a link.
fn make_dir_at(dir: &Dir, name: &OsStr) -> io::Result<Dir> {
    match dir.make_dir(name, false) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    dir.open_dir(name)
}

/// The folder at `path` under `dir`, each part made if missing; `dir`
/// itself for an empty path.
pub(crate) fn make_dirs_at(dir: &Dir, path: &Path) -> io::Result<Dir> {
    let mut current = dir.try_clone()?;
    for part in path.components() {
        current = make_dir_at(&current, part.as_os_str())?;
    }
    Ok(current)
}

/// A new file `name` in `dir`; it never replaces one or follows a link.
pub(crate) fn create_file_at(dir: &Dir, name: &OsStr, executable: bool) -> io::Result<File> {
    dir.create_file(name, executable)
}

/// Moves the item `name` out of `partial` into its target folder as
/// `name`, or the first free numbered name after it, and gives where it
/// landed.
pub(crate) fn place(partial: &Partial, name: &str, kind: ContentKind) -> Result<PathBuf> {
    let target = &partial.target;
    for n in 1..=MAX_SUFFIX {
        let numbered = numbered(name, kind, n);
        match fsat::rename_new(
            &partial.dir,
            OsStr::new(name),
            &target.dir,
            OsStr::new(&numbered),
            kind == ContentKind::Folder,
        ) {
            Ok(()) => return Ok(target.path.join(numbered)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("moving it to {}", target.path.join(&numbered).display())
                });
            }
        }
    }
    bail!(
        "{name} and {MAX_SUFFIX} numbered names after it are taken in {}",
        target.path.display()
    )
}

/// The path a journal entry holds, as `start` wrote it.
#[cfg(unix)]
fn recorded_path(bytes: Vec<u8>) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

/// The path a journal entry holds, as `start` wrote it. Windows paths the
/// node makes come from text, so they are UTF-8.
#[cfg(windows)]
fn recorded_path(bytes: Vec<u8>) -> io::Result<PathBuf> {
    String::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|_| io::Error::from(ErrorKind::InvalidData))
}

/// The partial folders being written, one file each under `dir` holding
/// the folder's path, so a node that was killed removes them next start.
#[derive(Debug, Clone)]
pub(crate) struct Journal {
    dir: PathBuf,
}

impl Journal {
    pub(crate) fn open(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).with_context(|| format!("making {}", dir.display()))?;
        Ok(Self { dir })
    }

    /// Removes the partial folders a stopped node left, and their entries.
    /// An entry that cannot be read is logged and left.
    pub(crate) fn sweep(&self) -> Result<()> {
        for entry in fs::read_dir(&self.dir)? {
            let entry = match entry {
                Ok(entry) => entry.path(),
                Err(e) => {
                    log::warn!("skipped a journal entry in {}: {e}", self.dir.display());
                    continue;
                }
            };
            let recorded = match fs::read(&entry).and_then(recorded_path) {
                Ok(path) => path,
                Err(e) => {
                    log::warn!("skipped the journal entry {}: {e}", entry.display());
                    continue;
                }
            };
            let ours = recorded
                .file_name()
                .map(|name| name.to_string_lossy())
                .is_some_and(|name| name.starts_with(PARTIAL_START) && name.ends_with(PARTIAL_END));
            if ours {
                // std's removal does not follow a link inside the folder.
                match fs::remove_dir_all(&recorded) {
                    Ok(()) => log::info!("removed a partial transfer: {}", recorded.display()),
                    Err(e) if e.kind() == ErrorKind::NotFound => {}
                    Err(e) => {
                        log::warn!("could not remove {}: {e}", recorded.display());
                        continue;
                    }
                }
            }
            if let Err(e) = fs::remove_file(&entry) {
                log::warn!(
                    "could not remove the journal entry {}: {e}",
                    entry.display()
                );
            }
        }
        Ok(())
    }

    /// Makes a hidden partial folder in `target`.
    pub(crate) fn start(&self, target: Arc<Folder>) -> Result<Partial> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos());
        let id = format!(
            "{:x}-{:x}-{nanos:x}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let name = format!("{PARTIAL_START}{id}{PARTIAL_END}");
        let entry = self.dir.join(&id);
        fs::write(
            &entry,
            target.path.join(&name).as_os_str().as_encoded_bytes(),
        )
        .with_context(|| format!("writing {}", entry.display()))?;
        let made = target
            .dir
            .make_dir(OsStr::new(&name), true)
            .and_then(|()| target.dir.open_dir(OsStr::new(&name)));
        let dir = match made {
            Ok(dir) => dir,
            Err(e) => {
                // The entry would only make the next start look for nothing.
                let _ = fs::remove_file(&entry);
                return Err(e)
                    .with_context(|| format!("making {}", target.path.join(&name).display()));
            }
        };
        Ok(Partial {
            target,
            name,
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
pub(crate) struct Partial {
    target: Arc<Folder>,
    name: String,
    dir: Dir,
    entry: PathBuf,
    done: bool,
}

impl Partial {
    /// The open partial folder, which the item is written into.
    pub(crate) fn dir(&self) -> &Dir {
        &self.dir
    }

    /// Forgets the folder once its item was moved out; it is empty.
    pub(crate) fn finish(mut self) -> Result<()> {
        self.done = true;
        self.target
            .dir
            .remove_dir(OsStr::new(&self.name))
            .with_context(|| format!("removing {}", self.name))?;
        fs::remove_file(&self.entry).with_context(|| format!("removing {}", self.entry.display()))
    }

    /// Removes the folder and everything written into it.
    pub(crate) fn remove(mut self) -> Result<()> {
        self.done = true;
        remove_partial(&self.target, &self.name, &self.entry)
    }
}

fn remove_partial(target: &Folder, name: &str, entry: &Path) -> Result<()> {
    target
        .dir
        .remove_tree(OsStr::new(name))
        .with_context(|| format!("removing {}", target.path.join(name).display()))?;
    fs::remove_file(entry).with_context(|| format!("removing {}", entry.display()))
}

impl Drop for Partial {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let (target, name, entry) = (self.target.clone(), self.name.clone(), self.entry.clone());
        let remove = move || {
            if let Err(e) = remove_partial(&target, &name, &entry) {
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

/// Bytes promised to the fetches running now, by disk, so fetches that
/// start together do not each count the same free space.
#[derive(Debug, Default)]
pub(crate) struct Room {
    reserved: Mutex<HashMap<u64, u64>>,
}

/// What one disk must hold for a fetch.
#[derive(Debug, Clone, Copy)]
struct Want {
    space: Space,
    bytes: u64,
}

/// Space a running fetch holds on its disks until it drops.
#[derive(Debug)]
pub(crate) struct Reserved {
    room: Arc<Room>,
    held: Vec<Want>,
}

impl Drop for Reserved {
    fn drop(&mut self) {
        let mut reserved = self.room.lock();
        for want in &self.held {
            if let Some(bytes) = reserved.get_mut(&want.space.device) {
                *bytes = bytes.saturating_sub(want.bytes);
                if *bytes == 0 {
                    reserved.remove(&want.space.device);
                }
            }
        }
    }
}

impl Room {
    fn lock(&self) -> MutexGuard<'_, HashMap<u64, u64>> {
        self.reserved.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Holds room for a fetch of `need` bytes: the store holds it once and
    /// the folder once, on one disk or two, with `margin` left on each
    /// beside what the other running fetches hold.
    pub(crate) fn reserve(
        self: &Arc<Self>,
        need: u64,
        store: Space,
        folder: Space,
        margin: u64,
    ) -> Result<Reserved> {
        let held = if store.device == folder.device {
            vec![Want {
                space: store,
                bytes: need.saturating_mul(2),
            }]
        } else {
            vec![
                Want {
                    space: store,
                    bytes: need,
                },
                Want {
                    space: folder,
                    bytes: need,
                },
            ]
        };
        let mut reserved = self.lock();
        for (at, want) in held.iter().enumerate() {
            let others = reserved.get(&want.space.device).copied().unwrap_or(0);
            let free = want.space.free.saturating_sub(others);
            if free >= want.bytes.saturating_add(margin) {
                continue;
            }
            let busy = if others > 0 {
                format!(" ({others} bytes are held for other transfers)")
            } else {
                String::new()
            };
            match (held.len(), at) {
                (1, _) => bail!(
                    "not enough free space: the transfer needs {} bytes on the target's disk \
                     while it is written out, and {free} are free{busy}",
                    want.bytes
                ),
                (_, 0) => bail!(
                    "not enough free space for the node's store: {need} bytes needed, \
                     {free} free{busy}"
                ),
                _ => bail!(
                    "not enough free space in the target folder: {need} bytes needed, \
                     {free} free{busy}"
                ),
            }
        }
        for want in &held {
            let bytes = reserved.entry(want.space.device).or_default();
            *bytes = bytes.saturating_add(want.bytes);
        }
        drop(reserved);
        Ok(Reserved {
            room: self.clone(),
            held,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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

    fn target(dir: &Path, folder: &str) -> Arc<Folder> {
        let folders = Folders::new(&[dir.to_owned()]);
        Arc::new(open_target(&folders, dir.join(folder).to_str().unwrap()).unwrap())
    }

    /// Writes `data` as `name` in a new partial folder of `target`.
    fn written(journal: &Journal, target: &Arc<Folder>, name: &str, data: &str) -> Partial {
        let partial = journal.start(target.clone()).unwrap();
        let mut file = create_file_at(partial.dir(), OsStr::new(name), false).unwrap();
        file.write_all(data.as_bytes()).unwrap();
        partial
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

    #[cfg(unix)]
    #[test]
    fn the_target_folder_is_absolute_existing_and_shared() {
        let dir = temp_dir("target");
        let shared = dir.0.join("shared");
        fs::create_dir_all(shared.join("in/deep")).expect("made");
        fs::write(shared.join("file.txt"), "x").expect("written");
        let outside = dir.0.join("outside");
        fs::create_dir_all(&outside).expect("made");
        std::os::unix::fs::symlink(shared.join("in"), shared.join("link")).expect("linked");
        let folders = Folders::new(std::slice::from_ref(&shared));

        let found = open_target(&folders, shared.join("in/deep").to_str().unwrap()).unwrap();
        assert_eq!(found.path(), shared.join("in/deep"));
        let relative = open_target(&folders, "in").unwrap_err();
        assert!(format!("{relative:#}").contains("not an absolute path"));
        let missing = open_target(&folders, shared.join("nope").to_str().unwrap()).unwrap_err();
        assert!(
            format!("{missing:#}").contains("does not exist"),
            "{missing:#}"
        );
        assert!(open_target(&folders, shared.join("file.txt").to_str().unwrap()).is_err());
        let out = open_target(&folders, outside.to_str().unwrap()).unwrap_err();
        assert!(format!("{out:#}").contains("outside the shared folders"));
        // A link inside the shared folder resolves to where it points.
        let linked = open_target(&folders, shared.join("link").to_str().unwrap()).unwrap();
        assert_eq!(linked.path(), shared.join("in"));
    }

    #[test]
    fn placing_keeps_both_and_never_replaces() {
        let dir = temp_dir("keep-both");
        let folder = dir.0.join("in");
        fs::create_dir_all(&folder).expect("made");
        fs::write(folder.join("render.png"), "old").expect("written");
        fs::write(folder.join("render (2).png"), "older").expect("written");
        let journal = Journal::open(dir.0.join("journal")).unwrap();
        let target = target(&dir.0, "in");

        let partial = written(&journal, &target, "render.png", "new");
        let placed = place(&partial, "render.png", ContentKind::File).unwrap();
        partial.finish().unwrap();
        assert_eq!(placed, folder.join("render (3).png"));
        assert_eq!(
            fs::read_to_string(folder.join("render.png")).unwrap(),
            "old"
        );
        assert_eq!(fs::read_to_string(&placed).unwrap(), "new");

        // A folder never lands on a folder, empty or not, or on a file.
        fs::create_dir_all(folder.join("frames")).expect("made");
        fs::write(folder.join("frames (2)"), "a file").expect("written");
        let partial = journal.start(target.clone()).unwrap();
        let frames = make_dir_at(partial.dir(), OsStr::new("frames")).unwrap();
        create_file_at(&frames, OsStr::new("1.png"), false).unwrap();
        let placed = place(&partial, "frames", ContentKind::Folder).unwrap();
        partial.finish().unwrap();
        assert_eq!(placed, folder.join("frames (3)"));
        assert!(placed.join("1.png").is_file());
        assert_eq!(fs::read_dir(dir.0.join("journal")).unwrap().count(), 0);
    }

    #[test]
    fn a_partial_folder_fits_a_long_name_and_reads_as_partial() {
        let dir = temp_dir("long");
        fs::create_dir_all(dir.0.join("in")).expect("made");
        let journal = Journal::open(dir.0.join("journal")).unwrap();
        let target = target(&dir.0, "in");
        let long = format!("{}.bin", "r".repeat(250));
        let partial = written(&journal, &target, &long, "x");
        let hidden = entries(&dir.0.join("in"));
        assert!(
            hidden[0].starts_with(".beep-") && hidden[0].ends_with(".partial"),
            "{hidden:?}"
        );
        let placed = place(&partial, &long, ContentKind::File).unwrap();
        partial.finish().unwrap();
        assert_eq!(placed, dir.0.join("in").join(&long));
        assert_eq!(entries(&dir.0.join("in")), [long]);
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_stopped_nodes_partial_folders_are_swept() {
        let dir = temp_dir("sweep");
        let folder = dir.0.join("in");
        fs::create_dir_all(&folder).expect("made");
        let journal = Journal::open(dir.0.join("journal")).unwrap();
        let target = target(&dir.0, "in");
        let partial = written(&journal, &target, "big.bin", "half");
        // As if the node was killed: nothing removes it now.
        std::mem::forget(partial);
        assert_eq!(entries(&folder).len(), 1);
        // An entry that cannot be read is skipped, not fatal.
        fs::create_dir(dir.0.join("journal/unreadable")).expect("made");
        journal.sweep().unwrap();
        assert!(entries(&folder).is_empty());
        assert_eq!(entries(&dir.0.join("journal")), ["unreadable"]);

        let partial = journal.start(target.clone()).unwrap();
        let deep = make_dirs_at(partial.dir(), Path::new("a/b/c")).unwrap();
        create_file_at(&deep, OsStr::new("x"), false).unwrap();
        partial.remove().unwrap();
        assert!(entries(&folder).is_empty());
    }

    #[test]
    fn room_counts_one_disk_twice() {
        let disk = |free, device| Space { free, device };
        let fits = |need, store, folder| {
            Arc::new(Room::default())
                .reserve(need, store, folder, 10)
                .is_ok()
        };
        assert!(fits(40, disk(100, 1), disk(100, 1)));
        assert!(!fits(50, disk(100, 1), disk(100, 1)));
        assert!(fits(50, disk(100, 1), disk(100, 2)));
        assert!(!fits(95, disk(100, 1), disk(200, 2)));
        assert!(!fits(95, disk(200, 1), disk(100, 2)));
        assert!(!fits(u64::MAX, disk(100, 1), disk(100, 1)));
    }

    #[test]
    fn running_fetches_share_the_free_space() {
        let disk = |free, device| Space { free, device };
        let room = Arc::new(Room::default());
        let (store, folder) = (disk(1000, 1), disk(1000, 2));
        let held: Vec<_> = (0..4)
            .map(|_| room.reserve(200, store, folder, 100).unwrap())
            .collect();
        // Four hold 800 of each disk's 1000; a fifth would leave less
        // than the margin.
        let refused = room.reserve(200, store, folder, 100).unwrap_err();
        assert!(format!("{refused:#}").contains("held for other transfers"));
        // Another disk pair is not affected.
        let other = room
            .reserve(200, disk(1000, 3), disk(1000, 4), 100)
            .unwrap();
        drop(held);
        let big = room.reserve(800, store, folder, 100).unwrap();
        drop(other);
        assert_eq!(room.lock().len(), 2);
        drop(big);
        assert!(room.lock().is_empty());
    }
}
