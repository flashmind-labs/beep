//! The pictures in sessions' transcripts (`Images`), kept by `image_key` so
//! the server can ask for one by its `[image: image-<key>]` line, and queued
//! for the server's `FollowImages` stream.

use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::image_key;
use serde_json::Value;
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::Notify;

/// The most pictures and bytes kept for one session...
const SESSION_IMAGES: usize = 16;
const SESSION_BYTES: usize = 32 * 1024 * 1024;
/// ...and for all of them.
const MAX_IMAGES: usize = 64;
const MAX_BYTES: usize = 128 * 1024 * 1024;
/// The longest session file line read when looking for a picture.
const MAX_LINE: usize = 64 * 1024 * 1024;
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
/// own session file (`find_in_file`).
#[derive(Debug, Default)]
pub(crate) struct Images {
    kept: Mutex<Kept>,
    unsent: Mutex<Outbox>,
    /// Woken when a picture joins `unsent`.
    arrived: Notify,
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
        let mut outbox = self.outbox();
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
                let mut outbox = self.outbox();
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
        let mut kept = self.lock();
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

    /// The picture `key` of `session`, when kept; it is then the newest.
    pub(crate) fn get(&self, session: &str, key: &str) -> Option<Arc<Picture>> {
        let mut kept = self.lock();
        let held = kept.take(session, key)?;
        let picture = held.picture.clone();
        kept.bytes += picture.data.len();
        kept.held.push_back(held);
        Some(picture)
    }

    fn lock(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn outbox(&self) -> MutexGuard<'_, Outbox> {
        self.unsent.lock().unwrap_or_else(PoisonError::into_inner)
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

/// The picture `key` in the session file at `path`, one JSON object per
/// line: an image block, or a data URL in any text, such as Codex's
/// `input_image` or a prompt an agent inlined it in.
pub(crate) fn find_in_file(path: &Path, key: &str) -> io::Result<Option<Picture>> {
    let mut lines = BufReader::new(File::open(path)?);
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = (&mut lines)
            .take(u64::try_from(MAX_LINE).unwrap_or(u64::MAX))
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(None);
        }
        if line.last() != Some(&b'\n') && read == MAX_LINE {
            // Too long to read: skip the rest of it.
            lines.skip_until(b'\n')?;
            continue;
        }
        if !contains(&line, b"base64") {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if let Some(found) = find_in(&value, key) {
            return Ok(Some(found));
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The picture `key` anywhere in `value`.
fn find_in(value: &Value, key: &str) -> Option<Picture> {
    let wanted = |picture: &Picture| image_key(&picture.data) == key;
    match value {
        Value::Object(fields) => block_picture(value)
            .filter(wanted)
            .or_else(|| fields.values().find_map(|field| find_in(field, key))),
        Value::Array(items) => items.iter().find_map(|item| find_in(item, key)),
        Value::String(text) => urls_in(text).find(wanted),
        _ => None,
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
mod tests {
    use super::*;
    use serde_json::json;

    fn png(fill: u8, size: usize) -> Picture {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(size, fill);
        Picture {
            mime: "image/png".into(),
            data,
        }
    }

    #[test]
    fn a_session_keeps_only_its_newest_pictures() {
        let images = Images::default();
        let first = png(0, 64);
        images.keep("a1", first.clone());
        for fill in 1..=u8::try_from(SESSION_IMAGES).unwrap() {
            images.keep("a1", png(fill, 64));
        }
        assert_eq!(images.get("a1", &image_key(&first.data)), None);
        let newest = png(u8::try_from(SESSION_IMAGES).unwrap(), 64);
        assert!(images.get("a1", &image_key(&newest.data)).is_some());
        // Another session's are left alone.
        let other = png(0, 65);
        images.keep("b2", other.clone());
        images.keep("a1", png(200, 64));
        assert!(images.get("b2", &image_key(&other.data)).is_some());
    }

    #[test]
    fn all_sessions_together_stay_under_the_overall_limit() {
        let images = Images::default();
        let first = png(0, 64);
        images.keep("s0", first.clone());
        for n in 1..=MAX_IMAGES {
            images.keep(&format!("s{n}"), png(1, 64 + n));
        }
        assert_eq!(images.get("s0", &image_key(&first.data)), None);
        assert_eq!(images.lock().held.len(), MAX_IMAGES);
        let big = png(2, SESSION_BYTES + 1);
        images.keep("s0", big.clone());
        assert_eq!(images.get("s0", &image_key(&big.data)), None, "too big");
    }

    #[test]
    fn new_pictures_wait_for_the_server_a_bounded_few() {
        use futures::FutureExt;
        let images = Images::default();
        images.keep_and_send("a1", png(0, 64));
        images.keep_and_send("a1", png(0, 64));
        images.keep("a1", png(1, 64));
        let first = images.next_unsent().now_or_never().expect("waiting");
        assert_eq!(first.picture.data, png(0, 64).data);
        assert!(
            images.next_unsent().now_or_never().is_none(),
            "a picture the session had, or one only kept, is not sent"
        );
        for n in 0..MAX_UNSENT + 2 {
            images.keep_and_send(&format!("s{n}"), png(2, 64 + n));
        }
        let oldest = images.next_unsent().now_or_never().expect("waiting");
        assert_eq!(oldest.session, "s2", "the oldest went");
    }

    #[test]
    fn a_picture_is_found_in_a_session_file() {
        let dir = std::env::temp_dir().join(format!("beep-images-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let claude = png(3, 40);
        let codex = png(4, 40);
        let lines = [
            json!({"type": "user", "message": {"content": [
                {"type": "text", "text": "what is this"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png",
                    "data": STANDARD.encode(&claude.data)}}
            ]}}),
            json!({"type": "response_item", "payload": {"content": [
                {"type": "input_image", "image_url": format!("data:image/png;base64,{}", STANDARD.encode(&codex.data))}
            ]}}),
        ];
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(&path, text).unwrap();
        let found = |picture: &Picture| find_in_file(&path, &image_key(&picture.data)).unwrap();
        assert_eq!(found(&claude), Some(claude));
        assert_eq!(found(&codex), Some(codex));
        assert_eq!(found(&png(5, 40)), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
