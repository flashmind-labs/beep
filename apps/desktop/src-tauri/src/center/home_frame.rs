//! The HTML of the last Home, kept so the main window paints it before its
//! scripts load. The app puts it on the page at document start (`script`).

use crate::error::{DesktopError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(any(desktop, test))]
use std::{fs, io::ErrorKind};
use tauri::{AppHandle, Manager, Runtime};

const FILE: &str = "home-frame.json";
/// The most HTML a frame may hold, in bytes.
const MAX_HTML: usize = 1024 * 1024;
/// The page side: appends the frame and sets the skin once `#bar`, the
/// last element before the module script, is parsed. `FRAME` stands for the
/// frame as JSON.
#[cfg(desktop)]
const SCRIPT: &str = r#"(() => {
const frame = FRAME;
const append = () => {
  if (frame.skin !== null) document.documentElement.dataset.skin = frame.skin;
  document.body.insertAdjacentHTML("beforeend", frame.html);
  window.__BEEP_KEPT_FRAME_AT__ = Date.now();
};
if (document.getElementById("bar")) {
  append();
} else {
  const observer = new MutationObserver(() => {
    if (!document.getElementById("bar")) return;
    observer.disconnect();
    append();
  });
  observer.observe(document, { childList: true, subtree: true });
}
})();
"#;

/// What Home's window drew: the page's skin and the frame's HTML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeFrame {
    pub skin: Option<String>,
    pub html: String,
}

#[derive(Serialize, Deserialize)]
struct FrameFile {
    /// The app version that wrote it. The page's markup changes between
    /// versions, so another version's frame is not shown.
    app: String,
    frame: HomeFrame,
}

fn path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf> {
    Ok(app.path().app_data_dir()?.join(FILE))
}

#[cfg(any(desktop, test))]
/// The frame saved at `path` by app version `app`. A missing, unreadable or
/// other version's file gives `None`.
fn load_from(path: &Path, app: &str) -> Option<HomeFrame> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!("reading the kept Home frame: {e}");
            return None;
        }
    };

    match serde_json::from_slice::<FrameFile>(&bytes) {
        Ok(file) if file.app == app => Some(file.frame),
        Ok(_) => None,
        Err(e) => {
            log::warn!("parsing the kept Home frame: {e}");
            None
        }
    }
}

/// The frame the last run kept, for this version of the app.
#[cfg(desktop)]
pub(crate) fn load<R: Runtime>(app: &AppHandle<R>) -> Option<HomeFrame> {
    match path(app) {
        Ok(path) => load_from(&path, env!("CARGO_PKG_VERSION")),
        Err(e) => {
            log::warn!("finding the kept Home frame: {e}");
            None
        }
    }
}

/// Writes `frame` to `path` through a partial file, so a reader never sees
/// half of it.
fn save_to(path: &Path, app: &str, frame: &HomeFrame) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(&FrameFile {
        app: app.to_owned(),
        frame: frame.clone(),
    })?;
    crate::store::write_part(path, &bytes)
}

/// Keeps `frame` as the Home the next launch paints first.
#[tauri::command]
pub async fn keep_home_frame(app: AppHandle, frame: HomeFrame) -> Result<()> {
    if frame.html.len() > MAX_HTML {
        return Err(DesktopError::BadRequest(format!(
            "the Home frame is over {MAX_HTML} bytes"
        )));
    }

    let path = path(&app)?;
    tokio::task::spawn_blocking(move || save_to(&path, env!("CARGO_PKG_VERSION"), &frame))
        .await
        .map_err(anyhow::Error::from)??;
    Ok(())
}

/// The initialization script that puts `frame` on the page.
#[cfg(desktop)]
pub(crate) fn script(frame: &HomeFrame) -> String {
    // Serializing a struct of strings cannot fail.
    let json = serde_json::to_string(frame).unwrap_or_else(|_| String::from("null"));
    SCRIPT.replacen("FRAME", &json, 1)
}

#[cfg(test)]
#[path = "../tests/center_home_frame.rs"]
mod tests;
