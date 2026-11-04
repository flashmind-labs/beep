//! The Add a project sheet: checkouts and host repositories to pick from,
//! the folder choice, and importing a project onto a machine.

use super::{known, list::ProjectRow, needs_update};
use crate::{
    beep::{Service, epoch_ms},
    error::{DesktopError, Result},
};
use beep_proto::{
    ApiError, BranchPlace, FoundAgent, FoundPlace, FoundProject, GitHostKind, HostRepos,
    ImportProject, ListFoundFolders, ListHostRepos, ListMachineFolder, MachineFolder,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::sync::oneshot;

/// The server stops a clone after 30 minutes; this leaves it time to say so.
pub(super) const IMPORT_TIMEOUT: Duration = Duration::from_secs(31 * 60);

/// A branch the import sheet gives a worktree, on `machine_id` or, without
/// one, the machine the project lands on.
#[derive(Debug, Deserialize)]
pub struct WorktreeArg {
    branch: String,
    machine_id: Option<String>,
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
    let found = known(beep.call(ListFoundFolders {}).await)?;
    Ok(found.into_iter().flatten().map(FoundRow::from).collect())
}

/// One git host's repositories to clone, as the Add a project sheet lists
/// them.
#[derive(Debug, Serialize)]
pub struct HostReposRow {
    host: String,
    github: bool,
    repos: Vec<HostRepoRow>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HostRepoRow {
    /// `org/repo`.
    name: String,
    /// What `import_project` takes as the repository.
    url: String,
    /// In milliseconds since the Unix epoch.
    updated_at: u64,
}

impl From<HostRepos> for HostReposRow {
    fn from(h: HostRepos) -> Self {
        Self {
            github: matches!(h.kind, GitHostKind::GitHub {}),
            host: h.host,
            repos: h
                .repos
                .into_iter()
                .map(|r| HostRepoRow {
                    name: r.name,
                    url: r.url,
                    updated_at: epoch_ms(r.updated_at),
                })
                .collect(),
            error: h.error,
        }
    }
}

/// The user's repositories on their git hosts, to clone; none from a
/// server too old to list them.
#[tauri::command]
pub async fn host_repos(beep: Service<'_>) -> Result<Vec<HostReposRow>> {
    let hosts = known(beep.call(ListHostRepos {}).await)?;
    Ok(hosts
        .into_iter()
        .flatten()
        .map(HostReposRow::from)
        .collect())
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

/// Clones `repo` onto `machine_id`, or the machine the server picks, into
/// `folder` when the user chose one, else the machine's projects folder,
/// then gives each of `worktrees` its worktree.
#[tauri::command]
pub async fn import_project(
    beep: Service<'_>,
    repo: String,
    machine_id: Option<String>,
    folder: Option<String>,
    worktrees: Option<Vec<WorktreeArg>>,
    parent: Option<String>,
) -> Result<ProjectRow> {
    let machine_id = machine_id.filter(|id| !id.is_empty());
    let folder = folder.filter(|folder| !folder.trim().is_empty());
    let parent = parent.filter(|parent| !parent.trim().is_empty());
    let worktrees = worktrees
        .unwrap_or_default()
        .into_iter()
        .map(|w| BranchPlace {
            branch: w.branch,
            machine_id: w.machine_id.filter(|id| !id.is_empty()),
        })
        .collect();
    beep.call_within(
        ImportProject {
            repo,
            machine_id,
            folder,
            worktrees,
            parent,
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
pub(super) fn outdated(e: DesktopError) -> DesktopError {
    match needs_update(e, "before projects can be imported") {
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
    fn refusals_read_as_the_servers_sentence() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 166"));
        assert!(outdated(old).to_string().contains("needs an update"));
        let said = DesktopError::Api(ApiError::bad_request("build-box is offline."));
        assert_eq!(outdated(said).to_string(), "build-box is offline.");
    }
}
