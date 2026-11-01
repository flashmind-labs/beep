//! Folders held open, and file operations relative to them that never
//! follow a link. On Unix a folder is an open descriptor and every step is
//! `*at`, so a link swapped in mid-way cannot redirect a write. Windows has
//! no `*at` calls: a folder is its path, and each step refuses reparse
//! points (links and junctions) along it, which leaves a short window
//! between the check and the write.
//!
//! Also the small differences in file metadata: ids, times and modes.

use std::io;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

pub(crate) use imp::{
    Dir, ctime_ns, id_of_metadata, id_of_path, is_executable, mtime_ns, open_no_follow, private,
    rename, rename_new, set_mode, volume,
};

/// Which file on which disk. On Windows an id read from `Metadata` is
/// zero: std does not expose the file index there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileId {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

/// Free bytes on a disk, and which disk it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Space {
    pub(crate) free: u64,
    pub(crate) device: u64,
}

/// The disk a path is on: which one, and its free and total bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Volume {
    pub(crate) device: u64,
    pub(crate) free: u64,
    pub(crate) total: u64,
}

/// Whether `error`, from opening a folder, means the path is a file or a
/// link rather than a folder.
pub(crate) fn not_a_folder(error: &io::Error) -> bool {
    imp::not_a_folder(error)
}
