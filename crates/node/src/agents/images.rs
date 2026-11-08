//! The pictures in sessions' transcripts (`Images`), kept by `image_key` so
//! the server can ask for one by its `[image: image-<key>]` line, and queued
//! for the server's `FollowImages` stream.

use super::image_index::FileIndexes;
use crate::lock;
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::image_key;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io,
    ops::ControlFlow,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

/// The most pictures and bytes kept for one session...
const SESSION_IMAGES: usize = 16;
const SESSION_BYTES: usize = 32 * 1024 * 1024;
/// ...and for all of them.
const MAX_IMAGES: usize = 64;
const MAX_BYTES: usize = 128 * 1024 * 1024;
/// The longest session file line read when looking for a picture.
pub(super) const MAX_LINE: usize = 64 * 1024 * 1024;
/// The most pictures and bytes waiting for the server; the oldest go.
const MAX_UNSENT: usize = 32;
const MAX_UNSENT_BYTES: usize = 64 * 1024 * 1024;

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
    unsent: Mutex<Outbox>,
    /// Woken when a picture joins `unsent`.
    arrived: Notify,
    files: FileIndexes,
}

/// A picture of `session` for the server to keep.
#[derive(Debug, Clone)]
pub(crate) struct Unsent {
    pub(crate) session: String,
    pub(crate) picture: Arc<Picture>,
}

/// The pictures waiting for the server, oldest first, and their bytes.
#[derive(Debug, Default)]
struct Outbox {
    waiting: VecDeque<Unsent>,
    bytes: usize,
}

#[derive(Debug, Default)]
struct Kept {
    held: VecDeque<Held>,
    bytes: usize,
}

#[derive(Debug)]
struct Held {
    session: String,
    key: String,
    picture: Arc<Picture>,
}

impl Images {
    /// Keeps `picture` for `session`, dropping the oldest past the limits.
    /// One larger than a session's room is not kept.
    pub(crate) fn keep(&self, session: &str, picture: Picture) {
        self.hold(session, picture);
    }

    /// Keeps `picture` as `keep` does and, when `session` did not have it
    /// yet, has it wait for the server.
    pub(crate) fn keep_and_send(&self, session: &str, picture: Picture) {
        let Some(picture) = self.hold(session, picture) else {
            return;
        };
        let mut outbox = lock(&self.unsent);
        outbox.bytes += picture.data.len();
        outbox.waiting.push_back(Unsent {
            session: session.to_owned(),
            picture,
        });
        while outbox.waiting.len() > MAX_UNSENT || outbox.bytes > MAX_UNSENT_BYTES {
            let Some(dropped) = outbox.waiting.pop_front() else {
                break;
            };
            outbox.bytes -= dropped.picture.data.len();
        }
        drop(outbox);
        self.arrived.notify_one();
    }

    /// The oldest picture waiting for the server, once there is one.
    pub(crate) async fn next_unsent(&self) -> Unsent {
        loop {
            let arrived = self.arrived.notified();
            let next = {
                let mut outbox = lock(&self.unsent);
                let next = outbox.waiting.pop_front();
                if let Some(next) = &next {
                    outbox.bytes -= next.picture.data.len();
                }
                next
            };
            if let Some(next) = next {
                return next;
            }
            arrived.await;
        }
    }

    /// Keeps `picture`; gives it when `session` did not have it before.
    fn hold(&self, session: &str, picture: Picture) -> Option<Arc<Picture>> {
        if picture.data.len() > SESSION_BYTES {
            return None;
        }
        let key = image_key(&picture.data);
        let picture = Arc::new(picture);
        let mut kept = lock(&self.kept);
        let had = kept.take(session, &key).is_some();
        kept.bytes += picture.data.len();
        kept.held.push_back(Held {
            session: session.to_owned(),
            key,
            picture: picture.clone(),
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
        (!had).then_some(picture)
    }

    /// The picture `key` in the session file at `path`. Blocks: a session
    /// file can be hundreds of MiB.
    pub(crate) fn find_in_file(&self, path: &Path, key: &str) -> io::Result<Option<Picture>> {
        self.files.find(path, key)
    }

    /// The picture `key` of `session`, when kept; it is then the newest.
    pub(crate) fn get(&self, session: &str, key: &str) -> Option<Arc<Picture>> {
        let mut kept = lock(&self.kept);
        let held = kept.take(session, key)?;
        let picture = held.picture.clone();
        kept.bytes += picture.data.len();
        kept.held.push_back(held);
        Some(picture)
    }
}

impl Kept {
    fn take(&mut self, session: &str, key: &str) -> Option<Held> {
        let at = self
            .held
            .iter()
            .position(|held| held.session == session && held.key == key)?;
        let held = self.held.remove(at)?;
        self.bytes -= held.picture.data.len();
        Some(held)
    }

    /// How many pictures `session` has and their bytes.
    fn of(&self, session: &str) -> (usize, usize) {
        self.held
            .iter()
            .filter(|held| held.session == session)
            .fold((0, 0), |(count, bytes), held| {
                (count + 1, bytes + held.picture.data.len())
            })
    }

    /// Drops the oldest picture, of `session` only when given.
    fn drop_oldest(&mut self, session: Option<&str>) {
        let at = self
            .held
            .iter()
            .position(|held| session.is_none_or(|session| held.session == session));
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
