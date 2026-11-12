//! Files and pictures the windows hand to the user: saved to Downloads,
//! shown in Finder or copied to the clipboard.

use super::clipboard::ImageClipboard;
use crate::error::{DesktopError, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

/// What the windows show for a file or picture a machine no longer has.
pub(crate) const GONE: &str = "No longer available";

/// Shows a file saved to Downloads in Finder.
#[tauri::command]
pub fn reveal_file(app: AppHandle, path: PathBuf) -> Result<()> {
    let downloads = app.path().download_dir()?;

    if path.parent() != Some(downloads.as_path()) || !path.is_file() {
        return Err(DesktopError::BadRequest("not a saved file".into()));
    }

    crate::platform::reveal(&path)?;
    Ok(())
}

/// `name` in `dir`, or the first of `name 2`, `name 3` and so on that is
/// free, the way Finder numbers a copy.
pub(crate) fn free_name(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);

    if !path.exists() {
        return path;
    }

    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2..1000)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} {}{ext}", Uuid::new_v4())))
}

/// Saves a picture the bar holds as a data URL to Downloads, as `name`
/// with the extension its type gives, and returns where it went.
#[tauri::command]
pub fn save_picture(app: AppHandle, name: String, url: String) -> Result<PathBuf> {
    let (mime, data) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
        .filter(|(mime, _)| image_mime(mime))
        .ok_or_else(|| DesktopError::BadRequest("bad picture".into()))?;
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| DesktopError::BadRequest("bad picture".into()))?;
    let to = free_name(&app.path().download_dir()?, &picture_name(&name, mime));
    fs::write(&to, bytes).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Copies the renderer's PNG through the native clipboard, without webview permissions.
#[tauri::command]
pub async fn copy_diagram_image(
    clipboard: State<'_, Arc<ImageClipboard>>,
    png: String,
) -> Result<()> {
    let clipboard = clipboard.inner().clone();
    tauri::async_runtime::spawn_blocking(move || clipboard.copy_png(&png))
        .await
        .map_err(anyhow::Error::from)??;
    Ok(())
}

/// `name` as a file name, with the extension of `mime` when it has none.
fn picture_name(name: &str, mime: &str) -> String {
    let name = file_name(name);

    if name.contains('.') {
        return name;
    }

    let ext = match mime {
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        other => other.trim_start_matches("image/"),
    };
    format!("{name}.{ext}")
}

/// Whether `mime` is a picture type plain enough to put in a data URL.
pub(crate) fn image_mime(mime: &str) -> bool {
    mime.strip_prefix("image/").is_some_and(|sub| {
        !sub.is_empty()
            && sub
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
    })
}

/// `name` without anything that would leave the file's folder.
pub(crate) fn file_name(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .trim_start_matches('.');
    if base.is_empty() {
        "file".into()
    } else {
        base.into()
    }
}

#[cfg(test)]
#[path = "../tests/chat_files.rs"]
mod tests;
