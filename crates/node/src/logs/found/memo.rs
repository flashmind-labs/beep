//! What was last read from a session file (`Memo`), kept in memory while the
//! file's modification time and length stay the same, so listing again reads
//! only the files that changed.

use crate::lock;
use std::{
    collections::HashMap,
    fs::{self, Metadata},
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

/// What a file looked like when it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: SystemTime,
    len: u64,
}

impl TryFrom<&Metadata> for Stamp {
    type Error = std::io::Error;

    fn try_from(meta: &Metadata) -> Result<Self, Self::Error> {
        Ok(Self {
            modified: meta.modified()?,
            len: meta.len(),
        })
    }
}

fn stamp_of(file: &Path) -> Option<Stamp> {
    fs::metadata(file)
        .ok()
        .as_ref()
        .and_then(|meta| Stamp::try_from(meta).ok())
}

pub(crate) fn wal_path(path: &Path) -> PathBuf {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    PathBuf::from(wal)
}

fn non_empty_stamp_of(file: &Path) -> Option<Stamp> {
    stamp_of(file).filter(|stamp| stamp.len > 0)
}

/// Values read from files, by the first file's path, at most `cap`; past
/// that the half used longest ago is dropped.
#[derive(Debug)]
pub(crate) struct Memo<V> {
    cap: usize,
    kept: Mutex<Kept<V>>,
}

#[derive(Debug)]
struct Kept<V> {
    entries: HashMap<PathBuf, Entry<V>>,
    /// Counts lookups, to tell which entries were used last.
    clock: u64,
}

#[derive(Debug)]
struct Entry<V> {
    /// One per file read, `None` for one that was not there.
    stamps: Vec<Option<Stamp>>,
    value: V,
    used: u64,
}

impl<V: Clone> Memo<V> {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            cap,
            kept: Mutex::new(Kept {
                entries: HashMap::new(),
                clock: 0,
            }),
        }
    }

    /// How many values are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        lock(&self.kept).entries.len()
    }

    /// The value `read` makes from `path`, kept from the last time while
    /// the file is unchanged. A failed read is not kept.
    pub(crate) fn read<E>(&self, path: &Path, read: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        self.read_files(&[path], |_| read())
    }

    /// As `read`, giving `read` the value kept for the file as it was
    /// before it changed, when there is one, to bring up to date.
    pub(crate) fn read_since<E>(
        &self,
        path: &Path,
        read: impl FnOnce(Option<V>) -> Result<V, E>,
    ) -> Result<V, E> {
        self.read_files(&[path], read)
    }

    /// As `read`, for the SQLite database at `database`, kept while neither
    /// it nor a non-empty `-wal` file changes. An empty `-wal` holds no
    /// frames and can be created whenever a reader opens a WAL database.
    pub(crate) fn read_database<E>(
        &self,
        database: &Path,
        read: impl FnOnce() -> Result<V, E>,
    ) -> Result<V, E> {
        let wal = wal_path(database);
        let stamps = vec![stamp_of(database), non_empty_stamp_of(&wal)];
        self.read_with_stamps(database, stamps, |_| read())
    }

    /// A value made from `files`, kept while none of them changes, appears
    /// or goes. The first must be there for it to be kept.
    fn read_files<E>(
        &self,
        files: &[&Path],
        read: impl FnOnce(Option<V>) -> Result<V, E>,
    ) -> Result<V, E> {
        let stamps: Vec<Option<Stamp>> = files.iter().map(|file| stamp_of(file)).collect();
        let Some(key) = files.first() else {
            return read(None);
        };
        self.read_with_stamps(key, stamps, read)
    }

    fn read_with_stamps<E>(
        &self,
        key: &Path,
        stamps: Vec<Option<Stamp>>,
        read: impl FnOnce(Option<V>) -> Result<V, E>,
    ) -> Result<V, E> {
        if stamps.first().and_then(|stamp| *stamp).is_none() {
            return read(None);
        }
        let before = match self.kept_value(key, &stamps) {
            Ok(value) => return Ok(value),
            Err(before) => before,
        };

        let value = read(before)?;
        self.keep(key.to_path_buf(), stamps, value.clone());
        Ok(value)
    }

    /// The value kept for `key` while it has `stamps`, else the one kept
    /// for it before, if any.
    fn kept_value(&self, key: &Path, stamps: &[Option<Stamp>]) -> Result<V, Option<V>> {
        let mut kept = lock(&self.kept);
        kept.clock += 1;
        let now = kept.clock;
        let Some(entry) = kept.entries.get_mut(key) else {
            return Err(None);
        };

        if entry.stamps != stamps {
            return Err(Some(entry.value.clone()));
        }

        entry.used = now;
        Ok(entry.value.clone())
    }

    fn keep(&self, key: PathBuf, stamps: Vec<Option<Stamp>>, value: V) {
        let mut kept = lock(&self.kept);
        let used = kept.clock;
        kept.entries.insert(
            key,
            Entry {
                stamps,
                value,
                used,
            },
        );

        if kept.entries.len() > self.cap {
            let mut ages: Vec<u64> = kept.entries.values().map(|entry| entry.used).collect();
            let middle = ages.len() / 2;
            let (_, &mut oldest_kept, _) = ages.select_nth_unstable(middle);
            kept.entries.retain(|_, entry| entry.used >= oldest_kept);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/logs_found_memo.rs"]
mod tests;
