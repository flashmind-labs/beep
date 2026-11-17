//! The pictures in sessions' transcripts (`Images`), kept by `image_key` so
//! the apps can ask for one by its `[image: image-<key>]` line.

use beep_proto::SessionId;
use super::image_index::FileIndexes;
use crate::lock;
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::image_key;
use serde_json::Value;
use std::{
    cmp::Reverse,
    collections::VecDeque,
    fs, io,
    ops::ControlFlow,
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
/// The longest session file line read when looking for a picture.
pub(super) const MAX_LINE: usize = 64 * 1024 * 1024;

/// An image's bytes and type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picture {
    pub(crate) mime: String,
    pub(crate) data: Vec<u8>,
}

impl Picture {
    /// The transcript line that shows it.
    pub(crate) fn line(&self) -> String {
        image_line(&self.data)
    }
}

/// How a transcript shows an image: by its bytes' key, which the apps ask
/// the node for.
pub(crate) fn image_line(data: &[u8]) -> String {
    format!("[image: image-{}]", image_key(data))
}

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

/// The picture a data URL holds, `data:image/png;base64,...`.
pub(crate) fn data_url(url: &str) -> Option<Picture> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    picture_of(mime, data)
}

/// The picture `data`, in base64, holds when `mime` is an image type.
pub(crate) fn picture_of(mime: &str, data: &str) -> Option<Picture> {
    if !mime.starts_with("image/") {
        return None;
    }

    let data = STANDARD.decode(data.trim()).ok()?;
    Some(Picture {
        mime: mime.to_owned(),
        data,
    })
}

/// The picture of an image content block: Claude Code's
/// `{"type":"image","source":{"media_type","data"}}` or ACP's
/// `{"type":"image","mimeType","data"}`.
pub(crate) fn block_picture(block: &Value) -> Option<Picture> {
    if block["type"] != "image" {
        return None;
    }

    let source = &block["source"];
    let (mime, data) = match source["data"].as_str() {
        Some(data) => (source["media_type"].as_str()?, data),
        None => (block["mimeType"].as_str()?, block["data"].as_str()?),
    };
    let data = STANDARD.decode(data).ok()?;
    Some(Picture {
        mime: mime.to_owned(),
        data,
    })
}

/// Whether a session file's line can hold a picture, before it is parsed.
pub(super) fn has_base64(line: &[u8]) -> bool {
    line.windows(6).any(|window| window == b"base64")
}

/// The picture `key` anywhere in `value`.
pub(super) fn find_in(value: &Value, key: &str) -> Option<Picture> {
    let found = each_picture(value, &mut |picture| {
        if image_key(&picture.data) == key {
            ControlFlow::Break(picture)
        } else {
            ControlFlow::Continue(())
        }
    });

    match found {
        ControlFlow::Break(picture) => Some(picture),
        ControlFlow::Continue(()) => None,
    }
}

/// Gives `each` the pictures anywhere in `value`, one at a time, until it
/// says to stop.
pub(super) fn each_picture<B>(
    value: &Value,
    each: &mut impl FnMut(Picture) -> ControlFlow<B>,
) -> ControlFlow<B> {
    match value {
        Value::Object(fields) => {
            if let Some(picture) = block_picture(value) {
                each(picture)?;
            }

            fields
                .values()
                .try_for_each(|field| each_picture(field, each))
        }
        Value::Array(items) => items.iter().try_for_each(|item| each_picture(item, each)),
        Value::String(text) => urls_in(text).try_for_each(each),
        _ => ControlFlow::Continue(()),
    }
}

/// The pictures of the data URLs in `text`.
fn urls_in(text: &str) -> impl Iterator<Item = Picture> + '_ {
    text.match_indices("data:image/").filter_map(|(at, _)| {
        let rest = &text[at..];
        let end = rest
            .find(|c: char| c == ')' || c == '"' || c == '\'' || c.is_whitespace())
            .unwrap_or(rest.len());
        data_url(&rest[..end])
    })
}

#[cfg(test)]
#[path = "../tests/agents_images.rs"]
mod tests;
