//! A picture whose path was pasted into a composer, read so the bar can
//! attach it, as flsh does when a terminal pastes a dragged file's path.

use crate::error::{DesktopError, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Larger pictures are left as the pasted path.
const MAX_BYTES: u64 = 20 * 1024 * 1024;

/// A picture's kind, from its file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum ImageType {
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/gif")]
    Gif,
    #[serde(rename = "image/webp")]
    Webp,
}

impl ImageType {
    fn of(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PastedImage {
    name: String,
    mime: ImageType,
    /// The file's bytes in base64.
    data: String,
}

/// Reads the picture at `path` (`~/` is the home folder). Only PNG, JPEG,
/// GIF and WebP files are read.
#[tauri::command]
pub async fn read_pasted_image(path: String) -> Result<PastedImage> {
    let path = full_path(&path, env::var_os("HOME").map(PathBuf::from).as_deref())?;
    tauri::async_runtime::spawn_blocking(move || read(&path))
        .await
        .map_err(anyhow::Error::from)?
}

/// `typed` as a full path; anything else is refused.
fn full_path(typed: &str, home: Option<&Path>) -> Result<PathBuf> {
    let path = match (typed.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(typed),
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(DesktopError::BadRequest("not a full path".into()))
    }
}

fn read(path: &Path) -> Result<PastedImage> {
    let mime =
        ImageType::of(path).ok_or_else(|| DesktopError::BadRequest("not a picture".into()))?;
    let size = fs::metadata(path).map_err(anyhow::Error::from)?.len();
    if size > MAX_BYTES {
        return Err(DesktopError::BadRequest("picture too large".into()));
    }
    let bytes = fs::read(path).map_err(anyhow::Error::from)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_owned());
    Ok(PastedImage {
        name,
        mime,
        data: STANDARD.encode(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_full_paths_are_read() {
        let home = Path::new("/home/someone");
        assert_eq!(
            full_path("~/a.png", Some(home)).expect("a home path is full"),
            home.join("a.png")
        );
        assert_eq!(
            full_path("/tmp/a.png", None).expect("an absolute path is full"),
            PathBuf::from("/tmp/a.png")
        );
        assert!(full_path("a.png", Some(home)).is_err());
        assert!(full_path("~/a.png", None).is_err());
    }

    #[test]
    fn only_pictures_are_read() {
        assert_eq!(ImageType::of(Path::new("/x/A.JPG")), Some(ImageType::Jpeg));
        assert_eq!(ImageType::of(Path::new("/x/a.webp")), Some(ImageType::Webp));
        assert_eq!(ImageType::of(Path::new("/x/notes.txt")), None);
        assert!(read(Path::new("/x/notes.txt")).is_err());
    }

    #[test]
    fn a_picture_is_read_with_its_name_and_type() {
        let dir = env::temp_dir().join(format!("beep-pasted-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("the temp folder is writable");
        let file = dir.join("shot.png");
        fs::write(&file, b"png").expect("the temp folder is writable");
        let got = read(&file).expect("the picture was just written");
        fs::remove_dir_all(&dir).expect("the temp folder was just made");
        assert_eq!(got.name, "shot.png");
        assert_eq!(got.mime, ImageType::Png);
        assert_eq!(got.data, "cG5n");
    }
}
