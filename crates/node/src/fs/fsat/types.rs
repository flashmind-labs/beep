//! What the file operations report: a file's identity and its disk's space.

/// Which file on which disk. On Windows an id read from `Metadata` is
/// zero: std does not expose the file index there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileId {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

/// The disk a path is on: which one, and its free and total bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Volume {
    pub(crate) device: u64,
    pub(crate) free: u64,
    pub(crate) total: u64,
}
