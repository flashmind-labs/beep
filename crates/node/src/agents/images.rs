//! The pictures in sessions' transcripts (`Images`), kept by `image_key` so
//! the apps can ask for one by its `[image: image-<key>]` line.

use super::{image_index::FileIndexes, picture::Picture};
use beep_proto::SessionId;
use crate::lock;
use beep_proto::image_key;
use std::{
    cmp::Reverse,
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// The most subagent transcripts searched for a session's picture.
const MAX_SUBAGENT_FILES: usize = 64;
/// The most pictures and bytes kept for one session...
const SESSION_IMAGES: usize = 16;
const SESSION_BYTES: usize = 32 * 1024 * 1024;
/// ...and for all of them.
const MAX_IMAGES: usize = 64;
const MAX_BYTES: usize = 128 * 1024 * 1024;
/// The pictures kept, oldest first, bounded per session and overall. One
/// evicted, or lost when the node restarted, is found again in the agent's
/// own session file (`find_in_file`), through `files`.
#[derive(Debug, Default)]
pub(crate) struct Images {
    kept: Mutex<Kept>,
    files: FileIndexes,
}

#[derive(Debug, Default)]
struct Kept {
    held: VecDeque<Held>,
    bytes: usize,
}

#[derive(Debug)]
struct Held {
    session: SessionId,
    key: String,
    picture: Arc<Picture>,
}

impl Images {
    /// Keeps `picture` for `session`, dropping the oldest past the limits.
    /// One larger than a session's room is not kept.
    pub(crate) fn keep(&self, session: &SessionId, picture: Picture) {
        if picture.data.len() > SESSION_BYTES {
            return;
        }

        let key = image_key(&picture.data);
        let picture = Arc::new(picture);
        let mut kept = lock(&self.kept);
        kept.take(session, &key);
        kept.bytes += picture.data.len();
        kept.held.push_back(Held {
            session: session.clone(),
            key,
            picture,
        });

        loop {
            let (count, bytes) = kept.of(session);

            if count <= SESSION_IMAGES && bytes <= SESSION_BYTES {
                break;
            }

            kept.drop_oldest(Some(session));
        }

        while kept.held.len() > MAX_IMAGES || kept.bytes > MAX_BYTES {
            kept.drop_oldest(None);
        }
    }

    /// The picture `key` in the session file at `path`. Blocks: a session
    /// file can be hundreds of MiB.
    pub(crate) fn find_in_file(&self, path: &Path, key: &str) -> io::Result<Option<Picture>> {
        if let Some(found) = self.files.find(path, key)? {
            return Ok(Some(found));
        }

        // A Claude subagent's pictures reach its parent's live log but are
        // written only to `<session>/subagents/agent-<id>.jsonl`.
        for file in subagent_files(path) {
            if let Some(found) = self.files.find(&file, key)? {
                return Ok(Some(found));
            }
        }

        Ok(None)
    }

    /// The picture `key` of `session`, when kept; it is then the newest.
    pub(crate) fn get(&self, session: &SessionId, key: &str) -> Option<Arc<Picture>> {
        let mut kept = lock(&self.kept);
        let held = kept.take(session, key)?;
        let picture = held.picture.clone();
        kept.bytes += picture.data.len();
        kept.held.push_back(held);
        Some(picture)
    }
}

/// The transcripts of the subagents of the Claude session at `path`, newest
/// first, at most `MAX_SUBAGENT_FILES`.
fn subagent_files(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path.with_extension("").join("subagents")) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .map(|entry| {
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
            (Reverse(modified), entry.path())
        })
        .collect();

    files.sort();
    files.truncate(MAX_SUBAGENT_FILES);
    files.into_iter().map(|(_, path)| path).collect()
}

impl Kept {
    fn take(&mut self, session: &SessionId, key: &str) -> Option<Held> {
        let at = self
            .held
            .iter()
            .position(|held| held.session == *session && held.key == key)?;
        let held = self.held.remove(at)?;
        self.bytes -= held.picture.data.len();
        Some(held)
    }

    /// How many pictures `session` has and their bytes.
    fn of(&self, session: &SessionId) -> (usize, usize) {
        self.held
            .iter()
            .filter(|held| held.session == *session)
            .fold((0, 0), |(count, bytes), held| {
                (count + 1, bytes + held.picture.data.len())
            })
    }

    /// Drops the oldest picture, of `session` only when given.
    fn drop_oldest(&mut self, session: Option<&SessionId>) {
        let at = self
            .held
            .iter()
            .position(|held| session.is_none_or(|session| held.session == *session));
        if let Some(held) = at.and_then(|at| self.held.remove(at)) {
            self.bytes -= held.picture.data.len();
        }
    }
}

#[cfg(test)]
#[path = "../tests/agents_images.rs"]
mod tests;
