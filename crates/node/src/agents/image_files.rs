//! Image files an agent made during a turn, named in what it says
//! (`named_in`), written by its edits or saved by its commands, read when
//! the turn ends (`read`, `read_since`), only inside the session's folder.

use super::picture::Picture;
use crate::fs::paths;
use beep_proto::sniff_image;
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// The most image files shown for one turn.
pub(crate) const MAX_PER_TURN: usize = 8;
/// The largest image file read, so the caller can be sent it whole.
pub(crate) const MAX_FILE: usize = super::MAX_IMAGE;

/// The image types read, by file extension.
const TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
];

/// The image type of the file `path` names, by its extension.
fn mime_of(path: &str) -> Option<&'static str> {
    let (_, extension) = path.rsplit_once('.')?;
    TYPES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(extension))
        .map(|&(_, mime)| mime)
}

/// The paths of image files `text` names, as written: plain or in
/// Markdown, quoted or in backticks, absolute, relative or `file://`.
pub(crate) fn named_in(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || "()[]<>\"'`,;|*".contains(c))
        .map(|word| word.trim_end_matches(['.', ':', '!', '?']))
        .map(|word| word.strip_prefix("file://").unwrap_or(word))
        .filter(|word| !word.contains("://") && !word.starts_with("data:"))
        .filter(|word| !word.starts_with('~') && mime_of(word).is_some())
}

/// `path` as a file of the folder `cwd`, when it names an image file.
pub(crate) fn resolve(cwd: &Path, path: &str) -> Option<PathBuf> {
    mime_of(path)?;
    Some(cwd.join(path))
}

/// The pictures of the image files at `paths` (each already joined to
/// `cwd`) that are inside `cwd`, small enough, and hold the image their
/// extension says. Reads the disk: run it off the async workers.
pub(crate) fn read(cwd: &Path, paths: &[PathBuf]) -> Vec<Picture> {
    read_since(cwd, paths, SystemTime::UNIX_EPOCH)
}

/// `read`, leaving out files last written before `since`.
pub(crate) fn read_since(cwd: &Path, paths: &[PathBuf], since: SystemTime) -> Vec<Picture> {
    let Ok(cwd) = paths::canonicalize(cwd) else {
        return Vec::new();
    };
    paths
        .iter()
        .take(MAX_PER_TURN)
        .filter_map(|path| match read_one(&cwd, path, since) {
            Ok(picture) => picture,
            Err(e) => {
                log::debug!("image file not read; path={} error={e}", path.display());
                None
            }
        })
        .collect()
}

fn read_one(cwd: &Path, path: &Path, since: SystemTime) -> std::io::Result<Option<Picture>> {
    let Some(mime) = path.to_str().and_then(mime_of) else {
        return Ok(None);
    };
    // Resolves links too, so one cannot point outside the folder.
    let path = paths::canonicalize(path)?;

    if !path.starts_with(cwd) {
        return Ok(None);
    }

    let file = crate::fs::fsat::open_read(&path)?;
    let meta = file.metadata()?;
    let limit = u64::try_from(MAX_FILE).unwrap_or(u64::MAX);

    if !meta.is_file() || meta.len() > limit || meta.modified()? < since {
        return Ok(None);
    }

    let mut data = Vec::new();
    // A file that grew since is cut at the limit and then refused below.
    file.take(limit + 1).read_to_end(&mut data)?;

    if data.len() > MAX_FILE || !holds(mime, &data) {
        return Ok(None);
    }

    Ok(Some(Picture {
        mime: mime.to_owned(),
        data,
    }))
}

/// Whether `data` is the image `mime` says.
fn holds(mime: &str, data: &[u8]) -> bool {
    if mime == "image/svg+xml" {
        return data.windows(4).any(|window| window == b"<svg");
    }

    sniff_image(data) == Some(mime)
}

#[cfg(test)]
#[path = "../tests/agents_image_files.rs"]
mod tests;
