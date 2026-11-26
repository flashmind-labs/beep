//! Media files the caller asks for by path, such as a video, 3D model or
//! PDF a coding agent made, read a chunk at a time so the apps can show
//! them (`SessionsRequest::File`).

use anyhow::{Context, Result, bail};
use beep_proto::{AGENT_FILE_CHUNK, media_mime};
use std::{
    fs,
    io::{ErrorKind, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Part of a media file.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Chunk {
    /// The whole file's size.
    pub(super) size: u64,
    pub(super) mime: &'static str,
    /// At most `AGENT_FILE_CHUNK` bytes from the asked offset.
    pub(super) data: Vec<u8>,
}

/// Up to `AGENT_FILE_CHUNK` bytes of the media file `path` from `offset`;
/// `None` when there is no such file. `path` is absolute or starts `~/`,
/// for `home`. Blocks on the file system.
pub(super) fn read_chunk(path: &str, offset: u64, home: Option<&Path>) -> Result<Option<Chunk>> {
    let Some(mime) = media_mime(path) else {
        bail!("{path} is not a picture, video, audio file, 3D model or PDF");
    };
    let full = resolve(path, home)?;
    let mut file = match crate::fs::fsat::open_read(&full) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("could not open {path}")),
    };
    let meta = file
        .metadata()
        .with_context(|| format!("could not read {path}"))?;
    if !meta.is_file() {
        bail!("{path} is not a file");
    }

    let size = meta.len();

    let mut data = Vec::new();

    if offset < size {
        let want = (size - offset).min(AGENT_FILE_CHUNK as u64);
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.take(want).read_to_end(&mut data))
            .with_context(|| format!("could not read {path}"))?;
    }

    Ok(Some(Chunk { size, mime, data }))
}

/// `path` as a full path: `~/` is `home`, and anything else must already
/// be absolute.
pub(super) fn resolve(path: &str, home: Option<&Path>) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = home.context("this node has no home folder to read ~/ in")?;
        return Ok(home.join(rest));
    }

    let full = Path::new(path);

    if !full.is_absolute() {
        bail!("{path} is not an absolute path or one starting ~/");
    }

    Ok(full.to_path_buf())
}

/// Whether each of `paths` is there. Only a path known to be missing
/// counts as gone: one that cannot be looked at, or named wrongly, does
/// not. Blocks.
pub(super) fn present(paths: &[String], home: Option<&Path>) -> Vec<bool> {
    paths
        .iter()
        .map(|path| match resolve(path, home) {
            Ok(full) => match fs::symlink_metadata(full) {
                Ok(_) => true,
                Err(e) => e.kind() != ErrorKind::NotFound,
            },
            Err(_) => true,
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/agents_media_file.rs"]
mod tests;
