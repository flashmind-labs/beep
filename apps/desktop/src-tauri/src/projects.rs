//! Projects imported onto the user's machines: the Import a project sheet,
//! its folder choice, and the rows the sidebar, Home and the Projects page
//! list.

use crate::{
    agents::SessionRow,
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
};
use beep_proto::{
    ApiError, FoundAgent, FoundPlace, FoundProject, ImportProject, ImportedProject,
    ListFoundFolders, ListMachineFolder, ListProjectMachines, ListProjectSessions, MachineFolder,
    ProjectMachine,
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
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

/// A git checkout the user's coding agents worked in, as the Add a project
/// sheet lists it.
#[derive(Debug, Serialize)]
pub struct FoundRow {
    /// `org/repo`.
    name: String,
    /// What `import_project` takes as the repository.
    remote: String,
    places: Vec<FoundPlace>,
    agents: Vec<FoundAgent>,
    /// In milliseconds since the Unix epoch.
    last_activity: u64,
}

impl From<FoundProject> for FoundRow {
    fn from(p: FoundProject) -> Self {
        Self {
            name: p.project,
            remote: p.remote,
            places: p.places,
            agents: p.agents,
            last_activity: epoch_ms(p.last_activity),
        }
    }
}

/// The checkouts the user's coding agents worked in that are not projects
/// yet; none from a server too old to look.
#[tauri::command]
pub async fn found_projects(beep: Service<'_>) -> Result<Vec<FoundRow>> {
    match beep.call(ListFoundFolders {}).await {
        Ok(found) => Ok(found.into_iter().map(FoundRow::from).collect()),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// The sessions coding agents ran in `project`'s folders, newest first;
/// none from a server too old to list them.
#[tauri::command]
pub async fn project_sessions(beep: Service<'_>, project: String) -> Result<Vec<SessionRow>> {
    match beep.call(ListProjectSessions { project }).await {
        Ok(sessions) => Ok(sessions.into_iter().map(SessionRow::from).collect()),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// One of the user's machines as a project's page lists it.
#[derive(Debug, Serialize)]
pub struct ProjectMachineRow {
    id: String,
    name: String,
    online: bool,
    /// The project's folder there; `None` without a checkout.
    folder: Option<String>,
    /// The server is looking through it for a checkout now.
    checking: bool,
    /// In milliseconds since the Unix epoch.
    checked_at: Option<u64>,
}

impl From<ProjectMachine> for ProjectMachineRow {
    fn from(m: ProjectMachine) -> Self {
        Self {
            id: m.machine_id,
            name: m.machine_name,
            online: m.online,
            folder: m.folder,
            checking: m.checking,
            checked_at: m.checked_at.map(epoch_ms),
        }
    }
}

/// Every machine of the user's for `project`'s page: its folder there, or
/// whether the server is looking for one. `None` from a server too old to
/// say, and the page lists the imported folders as before.
#[tauri::command]
pub async fn project_machines(
    beep: Service<'_>,
    project: String,
) -> Result<Option<Vec<ProjectMachineRow>>> {
    match beep.call(ListProjectMachines { project }).await {
        Ok(machines) => Ok(Some(
            machines.into_iter().map(ProjectMachineRow::from).collect(),
        )),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Shows a project's folder on this Mac in Finder.
#[tauri::command]
pub fn open_folder(path: PathBuf) -> Result<()> {
    if !openable(&path) {
        return Err(DesktopError::BadRequest(
            "That folder is not on this Mac.".into(),
        ));
    }
    Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(anyhow::Error::from)?;
    Ok(())
}

/// Only an absolute folder that is there opens.
fn openable(path: &Path) -> bool {
    path.is_absolute() && path.is_dir()
}

/// The server did not know the request.
fn unknown_op(e: &DesktopError) -> bool {
    matches!(e, DesktopError::Api(ApiError::BadRequest { message }) if message.starts_with(UNKNOWN_OP))
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
    fn only_a_folder_that_is_there_opens() {
        let dir = std::env::temp_dir();
        assert!(openable(&dir));
        assert!(!openable(Path::new("relative/folder")));
        assert!(!openable(&dir.join("no-such-folder-for-beep")));
    }

    #[test]
    fn an_old_server_finds_nothing() {
        assert!(unknown_op(&DesktopError::Api(ApiError::bad_request(
            "unknown op 169"
        ))));
        assert!(!unknown_op(&DesktopError::Api(ApiError::bad_request(
            "offline"
        ))));
    }

    #[test]
    fn refusals_read_as_the_servers_sentence() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 166"));
        assert!(outdated(old).to_string().contains("needs an update"));
        let said = DesktopError::Api(ApiError::bad_request("build-box is offline."));
        assert_eq!(outdated(said).to_string(), "build-box is offline.");
    }
}
