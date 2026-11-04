//! Files and pictures sent in a chat: downloaded once into the cache, then
//! opened, saved to Downloads or drawn as a data URL.

use crate::{
    bar::app::open,
    beep::{Beep, Service},
    error::{DesktopError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::ApiError;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// What the bar shows for a file or picture the server no longer has.
pub(crate) const GONE: &str = "No longer available";

/// Whether `id` names a file and nothing else, so it is safe in a path.
fn file_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Downloads file `id` to `path` unless an earlier download left it there.
async fn cached(beep: &Beep, id: &str, path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    match beep.download(id, path).await {
        Err(DesktopError::Api(ApiError::NotFound {})) => Err(DesktopError::BadRequest(GONE.into())),
        other => other,
    }
}

/// File `id` in the cache, downloaded the first time it is asked for.
async fn cached_file(app: &AppHandle, beep: &Beep, id: &str, name: &str) -> Result<PathBuf> {
    if !file_id(id) {
        return Err(DesktopError::BadRequest("bad file".into()));
    }
    let dir = app.path().app_cache_dir()?.join("files").join(id);
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let path = dir.join(file_name(name));
    cached(beep, id, &path).await?;
    Ok(path)
}

/// Opens a file Beep sent, downloading it into the cache the first time.
#[tauri::command]
pub async fn open_file(app: AppHandle, beep: Service<'_>, id: String, name: String) -> Result<()> {
    open(cached_file(&app, &beep, &id, &name).await?)
}

/// Saves a file Beep sent to Downloads, under a name not taken there yet,
/// and returns where it went.
#[tauri::command]
pub async fn save_file(
    app: AppHandle,
    beep: Service<'_>,
    id: String,
    name: String,
) -> Result<PathBuf> {
    let from = cached_file(&app, &beep, &id, &name).await?;
    let to = free_name(&app.path().download_dir()?, &file_name(&name));
    fs::copy(&from, &to).map_err(anyhow::Error::from)?;
    Ok(to)
}

/// Shows a file `save_file` put in Downloads in Finder.
#[tauri::command]
pub fn reveal_file(app: AppHandle, path: PathBuf) -> Result<()> {
    let downloads = app.path().download_dir()?;
    if path.parent() != Some(downloads.as_path()) || !path.is_file() {
        return Err(DesktopError::BadRequest("not a saved file".into()));
    }
    Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(anyhow::Error::from)?;
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

/// A picture someone sent, as a data URL the bar can draw. Its bytes are
/// kept in the cache, so each one downloads once.
#[tauri::command]
pub async fn picture(
    app: AppHandle,
    beep: Service<'_>,
    id: String,
    mime: String,
) -> Result<String> {
    if !file_id(&id) || !image_mime(&mime) {
        return Err(DesktopError::BadRequest("bad picture".into()));
    }
    let dir = app.path().app_cache_dir()?.join("pictures");
    fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let path = dir.join(&id);
    cached(&beep, &id, &path).await?;
    let bytes = fs::read(&path).map_err(anyhow::Error::from)?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
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
mod tests {
    use super::{file_name, free_name, image_mime, picture_name};
    use std::{env, fs};
    use uuid::Uuid;

    #[test]
    fn image_mime_takes_picture_types_only() {
        assert!(image_mime("image/jpeg"));
        assert!(image_mime("image/svg+xml"));
        assert!(!image_mime("image/"));
        assert!(!image_mime("text/html"));
        assert!(!image_mime("image/png;base64,x"));
    }

    #[test]
    fn free_name_numbers_a_taken_name() {
        let dir = env::temp_dir().join(format!("beep-free-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("the temp folder is writable");
        assert_eq!(free_name(&dir, "chart.png"), dir.join("chart.png"));
        fs::write(dir.join("chart.png"), b"x").expect("the temp folder is writable");
        assert_eq!(free_name(&dir, "chart.png"), dir.join("chart 2.png"));
        fs::write(dir.join("notes"), b"x").expect("the temp folder is writable");
        assert_eq!(free_name(&dir, "notes"), dir.join("notes 2"));
        fs::remove_dir_all(&dir).expect("the temp folder is removable");
    }

    #[test]
    fn file_name_stays_in_its_folder() {
        assert_eq!(file_name("Pass.pdf"), "Pass.pdf");
        assert_eq!(file_name("../../etc/passwd"), "passwd");
        assert_eq!(file_name(".hidden"), "hidden");
        assert_eq!(file_name(".."), "file");
        assert_eq!(file_name(""), "file");
    }

    #[test]
    fn a_saved_picture_gets_its_types_extension() {
        assert_eq!(picture_name("image-0123", "image/png"), "image-0123.png");
        assert_eq!(picture_name("photo", "image/jpeg"), "photo.jpg");
        assert_eq!(picture_name("shot.png", "image/png"), "shot.png");
        assert_eq!(picture_name("../x", "image/webp"), "x.webp");
    }
}
