//! A picture, and finding the pictures in a session file's JSON.

use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::image_key;
use serde_json::Value;
use std::ops::ControlFlow;

/// The longest session file line read when looking for a picture.
pub(crate) const MAX_LINE: usize = 64 * 1024 * 1024;

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
pub(crate) fn has_base64(line: &[u8]) -> bool {
    line.windows(6).any(|window| window == b"base64")
}

/// The picture `key` anywhere in `value`.
pub(crate) fn find_in(value: &Value, key: &str) -> Option<Picture> {
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
pub(crate) fn each_picture<B>(
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
