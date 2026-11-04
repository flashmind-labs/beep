//! Folders as open descriptors, with `*at` calls through rustix.

use super::{FileId, Space, Volume};
use rustix::{
    fs::{
        AtFlags, FileType, Mode, OFlags, RenameFlags, fstat, fstatvfs, linkat, mkdirat, openat,
        renameat, renameat_with, statat, unlinkat,
    },
    io::Errno,
};
use std::{
    ffi::OsStr,
    fs::{File, Metadata, OpenOptions, Permissions},
    io::{self, ErrorKind},
    os::{
        fd::OwnedFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::Path,
};

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// An open folder.
#[derive(Debug)]
pub(crate) struct Dir {
    fd: OwnedFd,
}

impl Dir {
    /// Opens the folder at `path`; its last part must not be a link.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            fd: rustix::fs::open(path, DIR_FLAGS, Mode::empty())?,
        })
    }

    /// Flushes everything written to this folder's disk, at one flush however
    /// many files. Where there is no such call this does nothing, and each
    /// file is flushed on its own instead.
    pub(crate) fn sync_disk(&self) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        rustix::fs::syncfs(&self.fd)?;
        Ok(())
    }

    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            fd: self.fd.try_clone()?,
        })
    }

    /// The folder `name` in this one, not following a link.
    pub(crate) fn open_dir(&self, name: &OsStr) -> io::Result<Self> {
        Ok(Self {
            fd: openat(&self.fd, name, DIR_FLAGS, Mode::empty())?,
        })
    }

    /// Makes the folder `name`; `private` leaves it to its owner.
    pub(crate) fn make_dir(&self, name: &OsStr, private: bool) -> io::Result<()> {
        let mode = if private { 0o700 } else { 0o777 };
        Ok(mkdirat(&self.fd, name, Mode::from_raw_mode(mode))?)
    }

    /// A new file `name`; it never replaces one or follows a link.
    pub(crate) fn create_file(&self, name: &OsStr, executable: bool) -> io::Result<File> {
        let flags = OFlags::WRONLY
            .union(OFlags::CREATE)
            .union(OFlags::EXCL)
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC);
        // The umask still applies.
        let mode = Mode::from_raw_mode(if executable { 0o777 } else { 0o666 });
        Ok(File::from(openat(&self.fd, name, flags, mode)?))
    }

    /// What `name` is, `None` when missing; `InvalidInput` for a link.
    pub(crate) fn metadata(&self, name: &OsStr) -> io::Result<Option<Metadata>> {
        let flags = OFlags::RDONLY
            .union(OFlags::NOFOLLOW)
            .union(OFlags::NONBLOCK)
            .union(OFlags::CLOEXEC);
        match openat(&self.fd, name, flags, Mode::empty()) {
            Ok(fd) => Ok(Some(File::from(fd).metadata()?)),
            Err(Errno::NOENT) => Ok(None),
            // A link.
            Err(Errno::LOOP | Errno::MLINK) => Err(ErrorKind::InvalidInput.into()),
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn remove_file(&self, name: &OsStr) -> io::Result<()> {
        Ok(unlinkat(&self.fd, name, AtFlags::empty())?)
    }

    pub(crate) fn remove_dir(&self, name: &OsStr) -> io::Result<()> {
        Ok(unlinkat(&self.fd, name, AtFlags::REMOVEDIR)?)
    }

    /// Removes `name` and everything under it, never following a link.
    pub(crate) fn remove_tree(&self, name: &OsStr) -> io::Result<()> {
        let dir = match openat(&self.fd, name, DIR_FLAGS, Mode::empty()) {
            Ok(fd) => Self { fd },
            Err(Errno::NOENT) => return Ok(()),
            // A file, or a link.
            Err(Errno::NOTDIR | Errno::LOOP) => return self.remove_file(name),
            Err(e) => return Err(e.into()),
        };
        let mut names = Vec::new();
        for entry in rustix::fs::Dir::read_from(&dir.fd)? {
            let entry = entry?;
            let entry_name = entry.file_name().to_bytes();
            if entry_name == b"." || entry_name == b".." {
                continue;
            }
            let is_dir = match entry.file_type() {
                FileType::Directory => true,
                FileType::Unknown => statat(&dir.fd, entry.file_name(), AtFlags::SYMLINK_NOFOLLOW)
                    .is_ok_and(|stat| FileType::from_raw_mode(stat.st_mode) == FileType::Directory),
                _ => false,
            };
            names.push((OsStr::from_bytes(entry_name).to_owned(), is_dir));
        }
        for (entry, is_dir) in names {
            if is_dir {
                dir.remove_tree(&entry)?;
            } else {
                match unlinkat(&dir.fd, &entry, AtFlags::empty()) {
                    Ok(()) | Err(Errno::NOENT) => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        match unlinkat(&self.fd, name, AtFlags::REMOVEDIR) {
            Ok(()) | Err(Errno::NOENT) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Bytes free on its disk, and the disk's id.
    pub(crate) fn space(&self) -> io::Result<Space> {
        let stats = fstatvfs(&self.fd)?;
        Ok(Space {
            free: stats.f_bavail.saturating_mul(stats.f_frsize),
            device: self.id()?.device,
        })
    }

    /// The folder's own id.
    pub(crate) fn id(&self) -> io::Result<FileId> {
        let stat = fstat(&self.fd)?;
        // Device and inode numbers are unsigned on every platform the node
        // runs on.
        #[allow(clippy::unnecessary_cast)]
        Ok(FileId {
            device: stat.st_dev as u64,
            inode: stat.st_ino as u64,
        })
    }
}

/// Moves `from` in `from_dir` to `to` in `to_dir` only if nothing is
/// there; `AlreadyExists` otherwise. `folder` says what is moved, for
/// filesystems that cannot rename exclusively.
pub(crate) fn rename_new(
    from_dir: &Dir,
    from: &OsStr,
    to_dir: &Dir,
    to: &OsStr,
    folder: bool,
) -> io::Result<()> {
    let taken = || io::Error::from(ErrorKind::AlreadyExists);
    let (from_fd, to_fd) = (&from_dir.fd, &to_dir.fd);
    match renameat_with(from_fd, from, to_fd, to, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(Errno::EXIST | Errno::NOTEMPTY) => Err(taken()),
        // The filesystem cannot rename exclusively.
        Err(Errno::INVAL | Errno::NOSYS | Errno::NOTSUP) if !folder => {
            // A link fails if the name is taken; then the old name goes.
            match linkat(from_fd, from, to_fd, to, AtFlags::empty()) {
                Ok(()) => Ok(unlinkat(from_fd, from, AtFlags::empty())?),
                Err(Errno::EXIST) => Err(taken()),
                Err(e) => Err(e.into()),
            }
        }
        // No exclusive way for a folder: check, then rename. Another writer
        // could slip in between, which only such a filesystem allows.
        Err(Errno::INVAL | Errno::NOSYS | Errno::NOTSUP) => {
            if statat(to_fd, to, AtFlags::SYMLINK_NOFOLLOW).is_ok() {
                return Err(taken());
            }
            match renameat(from_fd, from, to_fd, to) {
                Ok(()) => Ok(()),
                Err(Errno::EXIST | Errno::NOTEMPTY) => Err(taken()),
                Err(e) => Err(e.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Moves `from` in `from_dir` to `to` in `to_dir`, replacing a file there.
pub(crate) fn rename(from_dir: &Dir, from: &OsStr, to_dir: &Dir, to: &OsStr) -> io::Result<()> {
    Ok(renameat(&from_dir.fd, from, &to_dir.fd, to)?)
}

/// The disk `path` is on, following links.
pub(crate) fn volume(path: &Path) -> io::Result<Volume> {
    let stats = rustix::fs::statvfs(path)?;
    Ok(Volume {
        device: id_of_path(path)?.device,
        free: stats.f_bavail.saturating_mul(stats.f_frsize),
        total: stats.f_blocks.saturating_mul(stats.f_frsize),
    })
}

pub(super) fn not_a_folder(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP))
}

pub(crate) fn id_of_metadata(metadata: &Metadata) -> FileId {
    FileId {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

/// The id of what `path` names, following links.
pub(crate) fn id_of_path(path: &Path) -> io::Result<FileId> {
    Ok(id_of_metadata(&std::fs::metadata(path)?))
}

pub(crate) fn mtime_ns(metadata: &Metadata) -> i64 {
    metadata.mtime().saturating_mul(1_000_000_000) + metadata.mtime_nsec()
}

/// When the file's inode last changed.
pub(crate) fn ctime_ns(metadata: &Metadata) -> i64 {
    metadata.ctime().saturating_mul(1_000_000_000) + metadata.ctime_nsec()
}

pub(crate) fn is_executable(metadata: &Metadata) -> bool {
    metadata.permissions().mode() & 0o111 != 0
}

/// Sets the file's Unix mode.
pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    std::fs::set_permissions(path, Permissions::from_mode(mode))
}

/// Opens `path` with `options`, failing if it is a link. Never waits: a
/// pipe with nothing on its other end fails or opens at once.
pub(crate) fn open_no_follow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    options
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Opens `path` to read. Never waits: a pipe with nothing on its other end
/// opens at once rather than when a writer comes, so check it is a file.
pub(crate) fn open_read(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

/// Makes a file `options` create readable by its owner only.
pub(crate) fn private(options: &mut OpenOptions) -> &mut OpenOptions {
    options.mode(0o600)
}
