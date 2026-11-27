//! Where the pictures of a session file are, so a lookup reads one line of
//! a file that can be hundreds of MiB. One scan per file: lookups during it
//! wait for it, and later ones read only what was appended.

use crate::lock;
use crate::logs::picture::{MAX_LINE, Picture, each_picture, find_in, has_base64};
use beep_proto::image_key;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    ops::ControlFlow,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// The session files indexed; the one used longest ago goes.
const MAX_FILES: usize = 8;

/// The indexes of the session files read lately, last used last.
#[derive(Debug, Default)]
pub(super) struct FileIndexes {
    files: Mutex<VecDeque<Arc<FileIndex>>>,
}

#[derive(Debug)]
struct FileIndex {
    path: PathBuf,
    scanned: Mutex<Scanned>,
}

/// What a scan of a file found. No picture's bytes are kept.
#[derive(Debug, Default)]
struct Scanned {
    /// Where the first line not read for good starts: a last line without
    /// its newline may still be being written, so it is read again.
    end: u64,
    /// The offset of the line holding each picture, by key.
    lines: HashMap<String, u64>,
    /// All the bytes scans read, for the tests to see what was read again.
    #[cfg(test)]
    read: u64,
}

impl FileIndexes {
    /// The picture `key` in the session file at `path`, one JSON object per
    /// line: an image block, or a data URL in any text, such as Codex's
    /// `input_image` or a prompt an agent inlined it in. Blocks on the file
    /// and on any scan of it.
    pub(super) fn find(&self, path: &Path, key: &str) -> io::Result<Option<Picture>> {
        let index = self.index_of(path);
        let Some(at) = index.line_of(key)? else {
            return Ok(None);
        };

        if let Some(found) = picture_at(path, at, key)? {
            return Ok(Some(found));
        }
        // The line does not hold it any more: the file was replaced.
        *lock(&index.scanned) = Scanned::default();

        match index.line_of(key)? {
            Some(at) => picture_at(path, at, key),
            None => Ok(None),
        }
    }

    /// The index of `path`, now the last used.
    fn index_of(&self, path: &Path) -> Arc<FileIndex> {
        let mut files = lock(&self.files);
        let index = files
            .iter()
            .position(|file| file.path == path)
            .and_then(|at| files.remove(at))
            .unwrap_or_else(|| {
                Arc::new(FileIndex {
                    path: path.to_owned(),
                    scanned: Mutex::default(),
                })
            });
        files.push_back(index.clone());

        if files.len() > MAX_FILES {
            files.pop_front();
        }

        index
    }
}

impl FileIndex {
    /// The offset of the line holding `key`, scanning what is new in the
    /// file only when it is not known yet.
    fn line_of(&self, key: &str) -> io::Result<Option<u64>> {
        let mut scanned = lock(&self.scanned);
        let file = File::open(&self.path)?;
        let len = file.metadata()?.len();

        if len < scanned.end {
            *scanned = Scanned::default();
        }

        if let Some(&at) = scanned.lines.get(key) {
            return Ok(Some(at));
        }

        if len > scanned.end {
            scanned.scan(file)?;
        }

        Ok(scanned.lines.get(key).copied())
    }
}

impl Scanned {
    /// Reads `file` from `end` to its end, one line in memory at a time.
    fn scan(&mut self, mut file: File) -> io::Result<()> {
        file.seek(SeekFrom::Start(self.end))?;
        let mut lines = BufReader::new(file);
        let mut line = Vec::new();
        let limit = u64::try_from(MAX_LINE).unwrap_or(u64::MAX);

        loop {
            line.clear();
            let read = (&mut lines).take(limit).read_until(b'\n', &mut line)?;

            if read == 0 {
                return Ok(());
            }

            let ended = line.last() == Some(&b'\n');

            if !ended && read == MAX_LINE {
                // Too long to read: skip the rest of it. What arrives after
                // reads as a line that is not JSON.
                let rest = lines.skip_until(b'\n')?;
                self.passed(read + rest);
                continue;
            }

            self.index_line(&line);

            if !ended {
                self.count_read(read);
                return Ok(());
            }

            self.passed(read);
        }
    }

    /// Notes the pictures in the line that starts at `end`.
    fn index_line(&mut self, line: &[u8]) {
        if !has_base64(line) {
            return;
        }

        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let at = self.end;
        let _: ControlFlow<()> = each_picture(&value, &mut |picture| {
            self.lines.entry(image_key(&picture.data)).or_insert(at);
            ControlFlow::Continue(())
        });
    }

    fn passed(&mut self, read: usize) {
        self.end += u64::try_from(read).unwrap_or(u64::MAX);
        self.count_read(read);
    }

    #[cfg(test)]
    fn count_read(&mut self, read: usize) {
        self.read += u64::try_from(read).unwrap_or(u64::MAX);
    }

    #[cfg(not(test))]
    fn count_read(&mut self, _: usize) {}
}

/// The picture `key` in the line at the offset `at` of the file at `path`.
fn picture_at(path: &Path, at: u64, key: &str) -> io::Result<Option<Picture>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(at))?;
    let mut line = Vec::new();
    BufReader::new(file)
        .take(u64::try_from(MAX_LINE).unwrap_or(u64::MAX))
        .read_until(b'\n', &mut line)?;
    let Ok(value) = serde_json::from_slice::<Value>(&line) else {
        return Ok(None);
    };
    Ok(find_in(&value, key))
}

#[cfg(test)]
#[path = "../tests/agents_image_index.rs"]
mod tests;
