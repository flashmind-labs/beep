//! What was last read from a session file, kept in memory while the file
//! stays as it was, so listing a project's sessions again reads only the
//! files that changed. A file counts as unchanged while its modification
//! time and length are; a SQLite store also names its `-wal` file, which
//! takes the writes first.

use std::{
    collections::HashMap,
    fs::{self, Metadata},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::SystemTime,
};

/// What a file looked like when it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: SystemTime,
    len: u64,
}

impl Stamp {
    fn of(meta: &Metadata) -> Option<Self> {
        Some(Self {
            modified: meta.modified().ok()?,
            len: meta.len(),
        })
    }
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

    /// The value `read` makes from `path`, kept from the last time while
    /// the file is unchanged. A failed read is not kept.
    pub(crate) fn read<E>(&self, path: &Path, read: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        self.read_files(&[path], read)
    }

    /// As `read`, for the SQLite database at `database`, kept while neither
    /// it nor its `-wal` file changes.
    pub(crate) fn read_database<E>(
        &self,
        database: &Path,
        read: impl FnOnce() -> Result<V, E>,
    ) -> Result<V, E> {
        let mut wal = database.as_os_str().to_owned();
        wal.push("-wal");
        self.read_files(&[database, Path::new(&wal)], read)
    }

    /// A value made from `files`, kept while none of them changes, appears
    /// or goes. The first must be there for it to be kept.
    fn read_files<E>(&self, files: &[&Path], read: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        let stamps: Vec<Option<Stamp>> = files
            .iter()
            .map(|file| fs::metadata(file).ok().as_ref().and_then(Stamp::of))
            .collect();
        let Some((key, Some(_))) = files.first().zip(stamps.first()) else {
            return read();
        };
        if let Some(value) = self.kept_value(key, &stamps) {
            return Ok(value);
        }
        let value = read()?;
        self.keep(key.to_path_buf(), stamps, value.clone());
        Ok(value)
    }

    fn kept_value(&self, key: &Path, stamps: &[Option<Stamp>]) -> Option<V> {
        let mut kept = self.lock();
        kept.clock += 1;
        let now = kept.clock;
        let entry = kept.entries.get_mut(key)?;
        if entry.stamps != stamps {
            return None;
        }
        entry.used = now;
        Some(entry.value.clone())
    }

    fn keep(&self, key: PathBuf, stamps: Vec<Option<Stamp>>, value: V) {
        let mut kept = self.lock();
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

    fn lock(&self) -> MutexGuard<'_, Kept<V>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, convert::Infallible, fs::File};

    fn counted(memo: &Memo<String>, path: &Path, reads: &Cell<u32>) -> String {
        let read = || {
            reads.set(reads.get() + 1);
            Ok::<_, Infallible>(fs::read_to_string(path).expect("the file"))
        };
        memo.read(path, read).expect("infallible")
    }

    #[test]
    fn an_unchanged_file_is_read_once_and_a_changed_one_again() {
        let root = tempfile::tempdir().expect("a temp folder");
        let path = root.path().join("a.jsonl");
        fs::write(&path, "one").expect("a file");
        let memo = Memo::new(8);
        let reads = Cell::new(0);
        assert_eq!(counted(&memo, &path, &reads), "one");
        assert_eq!(counted(&memo, &path, &reads), "one");
        assert_eq!(reads.get(), 1);
        fs::write(&path, "three").expect("a longer file");
        assert_eq!(counted(&memo, &path, &reads), "three");
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn a_failed_read_is_not_kept() {
        let root = tempfile::tempdir().expect("a temp folder");
        let path = root.path().join("a.db");
        fs::write(&path, "x").expect("a file");
        let memo: Memo<u32> = Memo::new(8);
        assert_eq!(memo.read(&path, || Err("locked")), Err("locked"));
        assert_eq!(memo.read(&path, || Ok::<_, &str>(7)), Ok(7));
        assert_eq!(memo.read(&path, || Err("locked")), Ok(7));
    }

    #[test]
    fn a_new_wal_file_counts_as_a_change() {
        let root = tempfile::tempdir().expect("a temp folder");
        let db = root.path().join("s.db");
        fs::write(&db, "x").expect("a database");
        let memo: Memo<u32> = Memo::new(8);
        assert_eq!(memo.read_database(&db, || Ok::<_, Infallible>(1)), Ok(1));
        assert_eq!(memo.read_database(&db, || Ok::<_, Infallible>(2)), Ok(1));
        File::create(root.path().join("s.db-wal")).expect("a wal");
        assert_eq!(memo.read_database(&db, || Ok::<_, Infallible>(3)), Ok(3));
    }

    #[test]
    fn past_the_cap_the_least_used_go() {
        let root = tempfile::tempdir().expect("a temp folder");
        let memo: Memo<usize> = Memo::new(4);
        let paths: Vec<PathBuf> = (0..5)
            .map(|at| {
                let path = root.path().join(format!("{at}.jsonl"));
                fs::write(&path, "x").expect("a file");
                path
            })
            .collect();
        for (at, path) in paths.iter().take(4).enumerate() {
            memo.read(path, || Ok::<_, Infallible>(at)).expect("kept");
        }
        // The first is used again, so it outlives the second and third.
        memo.read(&paths[0], || Ok::<_, Infallible>(99))
            .expect("kept");
        memo.read(&paths[4], || Ok::<_, Infallible>(4))
            .expect("kept");
        assert!(memo.len() <= 4, "{}", memo.len());
        assert_eq!(memo.read(&paths[0], || Ok::<_, Infallible>(99)), Ok(0));
        assert_eq!(memo.read(&paths[1], || Ok::<_, Infallible>(11)), Ok(11));
    }
}
