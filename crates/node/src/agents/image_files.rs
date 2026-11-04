//! Image files an agent made during a turn: ones it names by path in what
//! it says, that its edits wrote, or that its commands wrote and named. The node reads them once the turn
//! ends and shows them in the transcript like any other picture.
//!
//! Only files inside the session's folder are read, at most
//! `MAX_PER_TURN` a turn and `MAX_FILE` bytes each.

use super::images::Picture;
use beep_proto::sniff_image;
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// The most image files shown for one turn.
pub(crate) const MAX_PER_TURN: usize = 8;
/// The largest image file read, so the server can be sent it whole.
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
    let Ok(cwd) = cwd.canonicalize() else {
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
    let path = path.canonicalize()?;
    if !path.starts_with(cwd) {
        return Ok(None);
    }
    let file = crate::fsat::open_read(&path)?;
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
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn image_paths_are_found_however_they_are_written() {
        let text = "Saved the chart to `out/chart.png`. See ![plot](plots/a.SVG), \
            /srv/project/shot.jpeg and file:///srv/project/b.webp: done. \
            Not https://example.com/c.png, notes.txt or ~/d.png";
        let found: Vec<&str> = named_in(text).collect();
        assert_eq!(
            found,
            [
                "out/chart.png",
                "plots/a.SVG",
                "/srv/project/shot.jpeg",
                "/srv/project/b.webp"
            ]
        );
    }

    #[test]
    fn only_real_images_inside_the_folder_are_read() {
        let root = std::env::temp_dir().join(format!("beep-image-files-{}", std::process::id()));
        let cwd = root.join("project");
        fs::create_dir_all(cwd.join("out")).unwrap();
        let png = b"\x89PNG\r\n\x1a\nchart".to_vec();
        fs::write(cwd.join("out/chart.png"), &png).unwrap();
        fs::write(cwd.join("fake.png"), b"not a picture").unwrap();
        fs::write(cwd.join("logo.svg"), b"<?xml version=\"1.0\"?><svg/>").unwrap();
        fs::write(root.join("outside.png"), &png).unwrap();
        let mut big = png.clone();
        big.resize(MAX_FILE + 1, 0);
        fs::write(cwd.join("big.png"), &big).unwrap();
        let paths: Vec<PathBuf> = [
            "out/chart.png",
            "fake.png",
            "logo.svg",
            "../outside.png",
            "big.png",
            "missing.png",
        ]
        .iter()
        .filter_map(|path| resolve(&cwd, path))
        .collect();
        let read = read(&cwd, &paths);
        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read[0].data, png);
        assert_eq!(read[0].mime, "image/png");
        assert_eq!(read[1].mime, "image/svg+xml");
        fs::remove_dir_all(&root).unwrap();
    }
}
