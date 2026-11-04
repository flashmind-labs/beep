//! Where the pictures of a session file are, so a lookup reads one line of
//! a file that can be hundreds of MiB. One scan per file: lookups during it
//! wait for it, and later ones read only what was appended.

use super::images::{MAX_LINE, Picture, each_picture, find_in, has_base64};
use crate::lock;
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
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::json;
    use std::{fs, io::Write, sync::Barrier, thread};
    use tempfile::{TempDir, tempdir};

    fn png(fill: u8) -> Picture {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(64, fill);
        Picture {
            mime: "image/png".into(),
            data,
        }
    }

    fn key(fill: u8) -> String {
        image_key(&png(fill).data)
    }

    /// The line a Codex session keeps picture `fill` in.
    fn codex_line(fill: u8) -> String {
        let url = format!("data:image/png;base64,{}", STANDARD.encode(png(fill).data));
        let line = json!({"type": "response_item", "payload": {"content": [
            {"type": "input_image", "image_url": url}
        ]}});
        format!("{line}\n")
    }

    /// The line a Claude Code session keeps picture `fill` in, with others around it.
    fn claude_line(fill: u8) -> String {
        let line = json!({"type": "user", "message": {"content": [
            {"type": "text", "text": "what is this"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png",
                "data": STANDARD.encode(png(fill).data)}}
        ]}});
        format!("{line}\n")
    }

    fn session(lines: &[String]) -> (TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(&path, lines.concat()).unwrap();
        (dir, path)
    }

    fn append(path: &Path, text: &str) {
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn read_so_far(indexes: &FileIndexes, path: &Path) -> u64 {
        lock(&indexes.index_of(path).scanned).read
    }

    fn file_len(path: &Path) -> u64 {
        fs::metadata(path).unwrap().len()
    }

    #[test]
    fn a_hit_reads_the_picture_from_its_line() {
        let plain = "{\"type\":\"text\"}\n".to_owned();
        let (_dir, path) = session(&[plain, claude_line(1), codex_line(2)]);
        let indexes = FileIndexes::default();
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), Some(png(1)));
        assert_eq!(indexes.find(&path, &key(2)).unwrap(), Some(png(2)));
        assert_eq!(
            read_so_far(&indexes, &path),
            file_len(&path),
            "scanned once"
        );
    }

    #[test]
    fn what_was_appended_is_read_from_where_the_scan_stopped() {
        let (_dir, path) = session(&[codex_line(1)]);
        let indexes = FileIndexes::default();
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), Some(png(1)));
        append(&path, &codex_line(2));
        assert_eq!(indexes.find(&path, &key(2)).unwrap(), Some(png(2)));
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), Some(png(1)));
        assert_eq!(
            read_so_far(&indexes, &path),
            file_len(&path),
            "nothing read again"
        );
    }

    #[test]
    fn a_line_still_being_written_is_read_again() {
        let whole = codex_line(1);
        let (cut, rest) = whole.split_at(whole.len() / 2);
        let (_dir, path) = session(&[cut.to_owned()]);
        let indexes = FileIndexes::default();
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), None);
        append(&path, rest);
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), Some(png(1)));
    }

    #[test]
    fn a_last_line_without_its_newline_is_found() {
        let (_dir, path) = session(&[codex_line(1).trim_end().to_owned()]);
        let indexes = FileIndexes::default();
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), Some(png(1)));
    }

    #[test]
    fn a_file_that_got_shorter_is_scanned_anew() {
        let (_dir, path) = session(&[codex_line(1), codex_line(2)]);
        let indexes = FileIndexes::default();
        assert_eq!(indexes.find(&path, &key(2)).unwrap(), Some(png(2)));
        fs::write(&path, codex_line(3)).unwrap();
        assert_eq!(indexes.find(&path, &key(2)).unwrap(), None);
        assert_eq!(indexes.find(&path, &key(3)).unwrap(), Some(png(3)));
        assert_eq!(read_so_far(&indexes, &path), file_len(&path));
    }

    #[test]
    fn a_file_replaced_by_a_longer_one_is_scanned_anew() {
        let (_dir, path) = session(&[codex_line(1)]);
        let indexes = FileIndexes::default();
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), Some(png(1)));
        fs::write(&path, [codex_line(2), codex_line(1)].concat()).unwrap();
        assert_eq!(indexes.find(&path, &key(1)).unwrap(), Some(png(1)));
    }

    #[test]
    fn a_missing_key_is_none_and_does_not_read_again() {
        let (_dir, path) = session(&[codex_line(1)]);
        let indexes = FileIndexes::default();
        assert_eq!(indexes.find(&path, &key(9)).unwrap(), None);
        assert_eq!(indexes.find(&path, &key(8)).unwrap(), None);
        assert_eq!(read_so_far(&indexes, &path), file_len(&path));
    }

    #[test]
    fn concurrent_lookups_share_one_scan() {
        let lines: Vec<String> = (0..12).map(codex_line).collect();
        let (_dir, path) = session(&lines);
        let indexes = FileIndexes::default();
        let start = Barrier::new(12);
        thread::scope(|scope| {
            for fill in 0..12 {
                let (indexes, path, start) = (&indexes, &path, &start);
                scope.spawn(move || {
                    start.wait();
                    assert_eq!(indexes.find(path, &key(fill)).unwrap(), Some(png(fill)));
                });
            }
        });
        assert_eq!(
            read_so_far(&indexes, &path),
            file_len(&path),
            "scanned once"
        );
    }

    #[test]
    fn the_oldest_index_goes_past_the_limit() {
        let (_dir, path) = session(&[codex_line(1)]);
        let indexes = FileIndexes::default();
        indexes.find(&path, &key(1)).unwrap();
        for n in 0..MAX_FILES {
            indexes.index_of(Path::new(&format!("other-{n}")));
        }
        assert_eq!(lock(&indexes.files).len(), MAX_FILES);
        assert_eq!(read_so_far(&indexes, &path), 0, "read again from nothing");
    }
}
