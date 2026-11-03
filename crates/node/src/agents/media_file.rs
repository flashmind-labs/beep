//! Media files the server asks for by path, such as a video, 3D model or
//! PDF a coding agent made, read a chunk at a time so the apps can show
//! them (`SessionsRequest::File`).

use anyhow::{Context, Result, bail};
use beep_proto::{AGENT_FILE_CHUNK, MAX_AGENT_FILE, media_mime};
use std::{
    fs::File,
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
        bail!("{path} is not a picture, video, 3D model or PDF");
    };
    let full = resolve(path, home)?;
    let mut file = match File::open(&full) {
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
    if size > MAX_AGENT_FILE {
        bail!(
            "{path} is {} MB, over the {} MB that can be shown",
            size / (1024 * 1024),
            MAX_AGENT_FILE / (1024 * 1024)
        );
    }
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
fn resolve(path: &str, home: Option<&Path>) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn folder_with(name: &str, data: &[u8]) -> (TempDir, String) {
        let dir = TempDir::new().expect("a temp folder can be made");
        let path = dir.path().join(name);
        fs::write(&path, data).expect("the temp folder is writable");
        let path = path.to_str().expect("temp paths are UTF-8").to_owned();
        (dir, path)
    }

    #[test]
    fn reads_a_chunk_at_an_offset() {
        let (_dir, path) = folder_with("clip.mp4", b"0123456789");
        let chunk = read_chunk(&path, 4, None).unwrap().unwrap();
        assert_eq!(
            chunk,
            Chunk {
                size: 10,
                mime: "video/mp4",
                data: b"456789".to_vec(),
            }
        );
    }

    #[test]
    fn reads_under_home_for_a_tilde_path() {
        let (dir, _) = folder_with("model.glb", b"glTF");
        let chunk = read_chunk("~/model.glb", 0, Some(dir.path()))
            .unwrap()
            .unwrap();
        assert_eq!(chunk.mime, "model/gltf-binary");
        assert_eq!(chunk.data, b"glTF");
    }

    #[test]
    fn an_offset_past_the_end_gives_no_bytes() {
        let (_dir, path) = folder_with("doc.pdf", b"%PDF");
        let chunk = read_chunk(&path, 100, None).unwrap().unwrap();
        assert_eq!(chunk.size, 4);
        assert!(chunk.data.is_empty());
    }

    #[test]
    fn a_missing_file_is_none() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("gone.png");
        assert_eq!(read_chunk(path.to_str().unwrap(), 0, None).unwrap(), None);
    }

    #[test]
    fn refuses_other_types() {
        let (_dir, path) = folder_with("notes.txt", b"secret");
        let e = read_chunk(&path, 0, None).unwrap_err();
        assert!(e.to_string().contains("is not a picture"), "{e:#}");
    }

    #[test]
    fn refuses_a_folder() {
        let dir = TempDir::new().unwrap();
        let folder = dir.path().join("renders.mp4");
        fs::create_dir(&folder).unwrap();
        let e = read_chunk(folder.to_str().unwrap(), 0, None).unwrap_err();
        assert!(e.to_string().contains("is not a file"), "{e:#}");
    }

    #[test]
    fn refuses_relative_paths() {
        let e = read_chunk("out/clip.mp4", 0, None).unwrap_err();
        assert!(e.to_string().contains("not an absolute path"), "{e:#}");
    }
}
