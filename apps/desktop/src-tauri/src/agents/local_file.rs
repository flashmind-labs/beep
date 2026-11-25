//! Open, Open With and Reveal in Finder for a file of a session's folder,
//! when this Mac has the file. The page names the folder, a path inside it
//! and an app from the list it was given; every path and bundle used here is
//! resolved in this module, never taken from the page.

#[cfg(target_os = "macos")]
use std::process::Command;

use crate::{
    error::{DesktopError, Result},
    fleet::{Machines, work_copy},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// An app macOS says can open a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenApp {
    /// What `open_local_file` takes back; the app's bundle.
    id: String,
    name: String,
    /// The app macOS opens the file with by default.
    default: bool,
}

/// A file of the session here, and the apps that can open it, the default first.
#[derive(Debug, Serialize)]
pub struct LocalFile {
    apps: Vec<OpenApp>,
}

/// What to do with a file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Open,
    Reveal,
    /// Open with the app of this id, one `local_file` listed.
    With(String),
}

/// `rel` inside `root`, as a path that exists and stays inside `root` once
/// links resolve; `None` for anything else: an absolute `rel`, `..` out of
/// the folder, a link that leads out, or a file that is not there.
fn inside(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);

    if !rel.is_relative() {
        return None;
    }

    let root = root.canonicalize().ok()?;
    let file = root.join(rel).canonicalize().ok()?;
    file.starts_with(&root).then_some(file)
}

/// The file `path` under `root` on `machine_id`, here on this Mac.
async fn resolve(
    fleet: &Machines<'_>,
    machine_id: &str,
    root: &str,
    path: &str,
) -> Option<PathBuf> {
    let folder = work_copy::exact_folder_here(fleet, machine_id, root).await?;
    let path = path.to_owned();
    spawn_blocking(move || inside(&folder, &path))
        .await
        .ok()
        .flatten()
}

/// Whether `path` under `root` on `machine_id` is a file on this Mac, and the
/// apps that can open it; `None` when it is not here.
#[tauri::command]
pub async fn local_file(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    path: String,
) -> Result<Option<LocalFile>> {
    let Some(file) = resolve(&fleet, &machine_id, &root, &path).await else {
        return Ok(None);
    };
    let apps = spawn_blocking(move || apps_for(&file))
        .await
        .map_err(|e| DesktopError::Other(e.into()))?;
    Ok(Some(LocalFile { apps }))
}

/// Opens or reveals the file `local_file` found for the same arguments.
#[tauri::command]
pub async fn open_local_file(
    fleet: Machines<'_>,
    machine_id: String,
    root: String,
    path: String,
    action: Action,
) -> Result<()> {
    let file = resolve(&fleet, &machine_id, &root, &path)
        .await
        .ok_or_else(|| DesktopError::BadRequest("that file is not on this computer".into()))?;
    spawn_blocking(move || {
        #[cfg(not(target_os = "macos"))]
        return match action {
            Action::Open => crate::platform::open(&file),
            Action::Reveal => crate::platform::reveal(&file),
            Action::With(id) => Err(DesktopError::BadRequest(format!(
                "app {id:?} is not available here"
            ))),
        };
        #[cfg(target_os = "macos")]
        {
            let mut cmd = Command::new("open");

            match action {
                Action::Open => {}
                Action::Reveal => {
                    cmd.arg("-R");
                }
                Action::With(id) => {
                    let app = apps_for(&file)
                        .into_iter()
                        .find(|a| a.id == id)
                        .ok_or_else(|| {
                            DesktopError::BadRequest("that app cannot open the file".into())
                        })?;
                    cmd.arg("-a").arg(app.id);
                }
            }

            let status = cmd.arg(&file).status().map_err(anyhow::Error::from)?;
            if status.success() {
                Ok(())
            } else {
                Err(DesktopError::BadRequest(format!("open failed: {status}")))
            }
        }
    })
    .await
    .map_err(|e| DesktopError::Other(e.into()))?
}

/// The apps LaunchServices says can open `file`, the default first, then by name.
#[cfg(target_os = "macos")]
fn apps_for(file: &Path) -> Vec<OpenApp> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    let Some(file) = file.to_str() else {
        return Vec::new();
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str(file));
    let workspace = NSWorkspace::sharedWorkspace();
    let default = workspace
        .URLForApplicationToOpenURL(&url)
        .and_then(|app| app.path())
        .map(|path| path.to_string());
    let mut apps: Vec<OpenApp> = Vec::new();

    for app in workspace.URLsForApplicationsToOpenURL(&url).iter() {
        let Some(bundle) = app.path().map(|path| path.to_string()) else {
            continue;
        };

        if apps.iter().any(|a| a.id == bundle) {
            continue;
        }

        let name = Path::new(&bundle)
            .file_stem()
            .map_or_else(|| bundle.clone(), |n| n.to_string_lossy().into_owned());
        apps.push(OpenApp {
            default: default.as_deref() == Some(bundle.as_str()),
            id: bundle,
            name,
        });
    }

    apps.sort_by(|a, b| b.default.cmp(&a.default).then_with(|| a.name.cmp(&b.name)));
    apps
}

#[cfg(not(target_os = "macos"))]
fn apps_for(_file: &Path) -> Vec<OpenApp> {
    Vec::new()
}

#[cfg(test)]
#[path = "../tests/agents_local_file.rs"]
mod tests;
