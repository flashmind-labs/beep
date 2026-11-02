//! Projects imported onto the user's machines: the Import a project sheet,
//! its folder choice, and the rows the sidebar, Home and the Projects page
//! list.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
};
use beep_proto::{ApiError, ImportProject, ImportedProject, ListMachineFolder, MachineFolder};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::sync::oneshot;

type Service<'a> = State<'a, Arc<Beep>>;

/// What an old server answers a request it does not know.
const UNKNOWN_OP: &str = "unknown op";
/// The server stops a clone after 30 minutes; this leaves it time to say so.
const IMPORT_TIMEOUT: Duration = Duration::from_secs(31 * 60);

/// One imported project as the window shows it.
#[derive(Debug, Serialize)]
pub struct ProjectRow {
    /// `org/repo`.
    name: String,
    url: String,
    machine_id: String,
    machine_name: String,
    folder: String,
    cloned: bool,
    /// In milliseconds since the Unix epoch.
    imported_at: u64,
}

impl From<ImportedProject> for ProjectRow {
    fn from(p: ImportedProject) -> Self {
        Self {
            name: p.project,
            url: p.url,
            machine_id: p.machine_id,
            machine_name: p.machine_name,
            folder: p.folder,
            cloned: p.cloned,
            imported_at: epoch_ms(p.imported_at),
        }
    }
}

/// Clones `repo` onto `machine_id`, or the machine the server picks, into
/// `folder` when the user chose one, else the machine's projects folder.
#[tauri::command]
pub async fn import_project(
    beep: Service<'_>,
    repo: String,
    machine_id: Option<String>,
    folder: Option<String>,
) -> Result<ProjectRow> {
    let machine_id = machine_id.filter(|id| !id.is_empty());
    let folder = folder.filter(|folder| !folder.trim().is_empty());
    beep.call_within(
        ImportProject {
            repo,
            machine_id,
            folder,
        },
        IMPORT_TIMEOUT,
    )
    .await
    .map(ProjectRow::from)
    .map_err(outdated)
}

/// One folder on `machine_id`, for the sheet's folder browser and to see
/// whether a folder picked on this Mac is a checkout: `path`, or the
/// machine's workspace, else its home.
#[tauri::command]
pub async fn list_machine_folder(
    beep: Service<'_>,
    machine_id: String,
    path: Option<String>,
) -> Result<MachineFolder> {
    beep.call(ListMachineFolder { machine_id, path })
        .await
        .map_err(outdated)
}

/// The macOS folder picker, opened at `start`; `None` when the user
/// cancels.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, start: Option<String>) -> Result<Option<String>> {
    let (picked, chosen) = oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose a folder for the project")
        .set_can_create_directories(true);
    if let Some(start) = start.filter(|start| !start.is_empty()) {
        dialog = dialog.set_directory(PathBuf::from(start));
    }
    dialog.pick_folder(move |folder| {
        // The sheet may have closed meanwhile; then nobody waits for it.
        let _ = picked.send(folder);
    });
    let folder = chosen
        .await
        .map_err(|_| DesktopError::BadRequest("The folder picker closed.".into()))?;
    folder.map(folder_text).transpose()
}

/// A picked folder as the path the server takes.
fn folder_text(folder: FilePath) -> Result<String> {
    let path = folder
        .into_path()
        .map_err(|e| DesktopError::BadRequest(format!("That folder cannot be used: {e}")))?;
    path.into_os_string()
        .into_string()
        .map_err(|_| DesktopError::BadRequest("That folder's name is not valid text.".into()))
}

/// An old server's refusal, in words the sheet shows; any other refusal
/// is the server's own sentence.
fn outdated(e: DesktopError) -> DesktopError {
    match e {
        DesktopError::Api(ApiError::BadRequest { message }) if message.starts_with(UNKNOWN_OP) => {
            DesktopError::BadRequest(
                "The server needs an update before projects can be imported.".into(),
            )
        }
        DesktopError::Api(ApiError::BadRequest { message }) => DesktopError::BadRequest(message),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_read_as_the_servers_sentence() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 166"));
        assert!(outdated(old).to_string().contains("needs an update"));
        let said = DesktopError::Api(ApiError::bad_request("build-box is offline."));
        assert_eq!(outdated(said).to_string(), "build-box is offline.");
    }
}
