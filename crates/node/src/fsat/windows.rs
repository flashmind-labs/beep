//! Folders as paths on Windows. Every step checks the path it reaches is
//! not a link or junction before using it.

use super::{FileId, Space, Volume};
use std::{
    ffi::OsStr,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, ErrorKind},
    iter,
    mem::MaybeUninit,
    os::windows::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    GetDiskFreeSpaceExW, GetFileInformationByHandle, MoveFileExW,
};

/// 100ns intervals from 1601 to 1970, where Windows file times start.
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

/// A folder, by path.
#[derive(Debug)]
pub(crate) struct Dir {
    path: PathBuf,
}

impl Dir {
    /// The folder at `path`; its last part must not be a link.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ErrorKind::NotADirectory.into());
        }
        Ok(Self {
            path: path.to_owned(),
        })
    }

    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            path: self.path.clone(),
        })
    }

    pub(crate) fn open_dir(&self, name: &OsStr) -> io::Result<Self> {
        Self::open(&self.path.join(name))
    }

    pub(crate) fn make_dir(&self, name: &OsStr, _private: bool) -> io::Result<()> {
        // A new folder takes its parent's permissions, which under the
        // user's profile are the user's alone.
        fs::create_dir(self.path.join(name))
    }

    /// A new file `name`; it never replaces one or follows a link.
    pub(crate) fn create_file(&self, name: &OsStr, _executable: bool) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.path.join(name))
    }

    /// What `name` is, `None` when missing; `InvalidInput` for a link.
    pub(crate) fn metadata(&self, name: &OsStr) -> io::Result<Option<Metadata>> {
        match fs::symlink_metadata(self.path.join(name)) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(ErrorKind::InvalidInput.into())
            }
            Ok(metadata) => Ok(Some(metadata)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn remove_file(&self, name: &OsStr) -> io::Result<()> {
        let path = self.path.join(name);
        // A link to a folder is removed as a folder.
        if fs::symlink_metadata(&path)?.file_type().is_symlink_dir() {
            return fs::remove_dir(&path);
        }
        fs::remove_file(&path)
    }

    pub(crate) fn remove_dir(&self, name: &OsStr) -> io::Result<()> {
        fs::remove_dir(self.path.join(name))
    }

    /// Removes `name` and everything under it, never following a link.
    pub(crate) fn remove_tree(&self, name: &OsStr) -> io::Result<()> {
        let path = self.path.join(name);
        let result = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(&path),
            Ok(_) => self.remove_file(name),
            Err(e) => Err(e),
        };
        match result {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    pub(crate) fn space(&self) -> io::Result<Space> {
        let volume = volume(&self.path)?;
        Ok(Space {
            free: volume.free,
            device: volume.device,
        })
    }

    pub(crate) fn id(&self) -> io::Result<FileId> {
        id_of_path(&self.path)
    }
}

/// The disk `path` is on, following links.
pub(crate) fn volume(path: &Path) -> io::Result<Volume> {
    let wide = wide(path);
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `wide` is a NUL-terminated path that outlives the call,
    // `free` and `total` are valid u64s to write, and the last output may
    // be null.
    let ok =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Volume {
        device: id_of_path(path)?.device,
        free,
        total,
    })
}

/// `path` as a NUL-terminated UTF-16 string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

/// Moves `from` to `to` only if nothing is there; `AlreadyExists`
/// otherwise. Without `MOVEFILE_REPLACE_EXISTING` the move itself refuses
/// a taken name, for files and folders alike.
pub(crate) fn rename_new(
    from_dir: &Dir,
    from: &OsStr,
    to_dir: &Dir,
    to: &OsStr,
    _folder: bool,
) -> io::Result<()> {
    let (from, to) = (wide(&from_dir.path.join(from)), wide(&to_dir.path.join(to)));
    // SAFETY: both are NUL-terminated paths that outlive the call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Moves `from` to `to`, replacing a file there.
pub(crate) fn rename(from_dir: &Dir, from: &OsStr, to_dir: &Dir, to: &OsStr) -> io::Result<()> {
    fs::rename(from_dir.path.join(from), to_dir.path.join(to))
}

pub(super) fn not_a_folder(error: &io::Error) -> bool {
    error.kind() == ErrorKind::NotADirectory
}

/// Zero: std does not expose the file index on Windows. Paths that need
/// an id use `id_of_path`.
pub(crate) fn id_of_metadata(_metadata: &Metadata) -> FileId {
    FileId {
        device: 0,
        inode: 0,
    }
}

/// The volume serial number and file index of what `path` names.
pub(crate) fn id_of_path(path: &Path) -> io::Result<FileId> {
    // Backup semantics lets a folder open too.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the handle is open for the call, and `info` is a writable
    // struct of the type the call fills.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so it filled `info`.
    let info = unsafe { info.assume_init() };
    Ok(FileId {
        device: info.dwVolumeSerialNumber.into(),
        inode: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

/// A Windows file time in nanoseconds since 1970.
fn file_time_ns(ticks: u64) -> i64 {
    i64::try_from(ticks)
        .unwrap_or(i64::MAX)
        .saturating_sub(UNIX_EPOCH_TICKS)
        .saturating_mul(100)
}

pub(crate) fn mtime_ns(metadata: &Metadata) -> i64 {
    file_time_ns(metadata.last_write_time())
}

/// When the file was made: Windows keeps no inode change time that std
/// exposes, and a replaced file gets a new one.
pub(crate) fn ctime_ns(metadata: &Metadata) -> i64 {
    file_time_ns(metadata.creation_time())
}

/// Windows has no executable bit.
pub(crate) fn is_executable(_metadata: &Metadata) -> bool {
    false
}

/// Windows has no Unix modes; files under the user's profile are already
/// the user's alone.
pub(crate) fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// Opens `path` with `options`, failing if it is a link. The link itself
/// is opened, so what it points at is never touched, then refused.
pub(crate) fn open_no_follow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    let file = options
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_type().is_symlink() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{} is a link", path.display()),
        ));
    }
    Ok(file)
}

/// Opens `path` to read.
pub(crate) fn open_read(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// Files under the user's profile are already the user's alone.
pub(crate) fn private(options: &mut OpenOptions) -> &mut OpenOptions {
    options
}
