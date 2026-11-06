//! `files/1`: files on a node, inside the folders its owner shared in
//! `node.toml`. A node offers it only when it shares some. Each request gets
//! one `Reply`, in order, on a stream that stays open until the server closes
//! it. A path is absolute, or relative to the first shared folder; the node
//! refuses anything that resolves outside the shared folders, symlinks
//! included.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "files/1";

/// The most bytes one `Read` returns or one `Write` takes.
pub const MAX_CHUNK: u32 = 4 * 1024 * 1024;

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The folders the node shares.
    Roots {},
    /// Up to `limit` bytes of a file from `offset` (`limit` 0 means
    /// `MAX_CHUNK`).
    Read {
        path: String,
        #[serde(default)]
        offset: u64,
        #[serde(default)]
        limit: u32,
    },
    /// Writes `data` to a file, made if missing, replacing what it held
    /// unless `append`. Its folder must exist.
    Write {
        path: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        #[serde(default)]
        append: bool,
    },
    /// What a folder holds, by name, at most `limit` entries (0 for the
    /// node's own limit).
    List {
        path: String,
        #[serde(default)]
        limit: u32,
    },
    Stat {
        path: String,
    },
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    Roots {
        roots: Vec<String>,
    },
    Data {
        /// The path as the node resolved it.
        path: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
        /// The file's whole size.
        size: u64,
    },
    Written {
        path: String,
        /// The file's size afterwards.
        size: u64,
    },
    Entries {
        path: String,
        entries: Vec<FileEntry>,
        /// Entries left out past the limit.
        #[serde(default)]
        more: u32,
    },
    Stat {
        path: String,
        entry: FileEntry,
    },
    /// The request failed, such as a path outside the shared folders.
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    #[serde(default)]
    pub kind: FileKind,
    #[serde(default)]
    pub size: u64,
    /// Unix seconds.
    #[serde(default)]
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    #[default]
    #[serde(other)]
    Other,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Dir => "dir",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }
}
