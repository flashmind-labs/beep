//! The Add a project sheet: checkouts and host repositories to pick from,
//! the folder choice, and importing a project onto a machine.

use super::{list::ProjectRow, needs_update};
use crate::{
    beep::{Service, epoch_ms},
    error::{DesktopError, Result},
    fleet::{
        Machines, extra, files as direct,
        projects::{FleetState, desk, mirror_import},
    },
    machines::list::Outside,
};
use beep_fleet::ImportJob;
use beep_proto::{
    ApiError, BranchPlace, FoundAgent, FoundPlace, FoundProject, GitHostKind, HostRepos,
    ListFoundFolders, ListMachineFolder, MachineFolder,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::sync::oneshot;

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

/// The checkouts on the machines that are not projects yet, as each machine
/// named them last: at once, for `rescan_found` to bring up to date.
#[tauri::command]
pub async fn found_projects(
    beep: Service<'_>,
    fleet: Machines<'_>,
    session: FleetState<'_>,
    held: State<'_, Outside>,
) -> Result<Vec<FoundRow>> {
    Ok(grouped(&beep, &fleet, &session, &held).await)
}

/// A machine of the mesh to ask for its checkouts.
#[derive(Debug, Serialize)]
pub struct FoundMachine {
    id: String,
    name: String,
}

/// The machines of the mesh online now, this Mac among them.
#[tauri::command]
pub fn found_machines(fleet: Machines<'_>) -> Vec<FoundMachine> {
    fleet
        .online()
        .into_iter()
        .map(|machine| FoundMachine {
            id: machine.to_string(),
            name: fleet.name_of(machine),
        })
        .collect()
}

/// The found list after one machine looked again, and why it could not.
#[derive(Debug, Serialize)]
pub struct FoundAfter {
    rows: Vec<FoundRow>,
    /// In words, when the machine could not say what it has.
    problem: Option<String>,
}

/// Has `machine_id` walk its search folder again, quickly, and name its
/// checkouts; gives the found list with what it named.
#[tauri::command]
pub async fn rescan_found(
    beep: Service<'_>,
    fleet: Machines<'_>,
    session: FleetState<'_>,
    held: State<'_, Outside>,
    machine_id: String,
) -> Result<FoundAfter> {
    let machine = fleet
        .member(&machine_id)
        .ok_or_else(|| DesktopError::BadRequest("That machine is not in the mesh.".into()))?;
    let problem = match direct::found_on(&fleet, machine).await {
        Ok(found) => {
            held.found.keep(found);
            None
        }
        Err(why) => Some(why),
    };
    Ok(FoundAfter {
        rows: grouped(&beep, &fleet, &session, &held).await,
        problem,
    })
}

/// What the machines named last, without what the mesh already has as a
/// project, and what the server found on machines outside the mesh.
async fn grouped(
    beep: &Service<'_>,
    fleet: &Machines<'_>,
    session: &FleetState<'_>,
    held: &Outside,
) -> Vec<FoundRow> {
    let here = held.found.last();
    let mut found = match desk(session).await {
        Ok(desk) => desk.group_found(here),
        Err(_) => beep_projects::imports::found::group(here, &[], &[]),
    };
    let others = extra(beep.call(ListFoundFolders {}).await);
    found.extend(others.into_iter().filter(|project| {
        project
            .places
            .iter()
            .all(|place| fleet.member(&place.machine_id).is_none())
    }));
    found.into_iter().map(FoundRow::from).collect()
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

/// The user's repositories on their git hosts, to clone.
#[tauri::command]
pub async fn host_repos(fleet: FleetState<'_>) -> Result<Vec<HostReposRow>> {
    let hosts = desk(&fleet).await?.host_repos().await?;
    Ok(hosts.into_iter().map(HostReposRow::from).collect())
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

/// Clones `repo` onto `machine_id`, or the machine that fits best, into
/// `folder` when the user chose one, else the machine's projects folder,
/// then gives each of `worktrees` its worktree.
#[tauri::command]
pub async fn import_project(
    fleet: FleetState<'_>,
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
    let worktrees: Vec<BranchPlace> = worktrees
        .unwrap_or_default()
        .into_iter()
        .map(|w| BranchPlace {
            branch: w.branch,
            machine_id: w.machine_id.filter(|id| !id.is_empty()),
        })
        .collect();
    let imported = desk(&fleet)
        .await?
        .import(ImportJob {
            repo: &repo,
            machine_id: machine_id.as_deref(),
            folder: folder.as_deref(),
            parent: parent.as_deref(),
            worktrees: &worktrees,
        })
        .await?;
    mirror_import(&beep, &imported).await;
    Ok(imported.into())
}

/// One folder on `machine_id`, for the sheet's folder browser and to see
/// whether a folder picked on this Mac is a checkout: `path`, or the
/// machine's workspace, else its home.
#[tauri::command]
pub async fn list_machine_folder(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    path: Option<String>,
) -> Result<MachineFolder> {
    if let Some(machine) = fleet.member(&machine_id) {
        return direct::folder(&fleet, machine, path.as_deref()).await;
    }
    beep.call(ListMachineFolder { machine_id, path })
        .await
        .map_err(outdated)
}

/// The branch the checkout at `path` on `machine_id` is on; `None` when it
/// is no checkout or the machine is not in the mesh.
#[tauri::command]
pub async fn folder_branch(
    fleet: Machines<'_>,
    machine_id: String,
    path: String,
) -> Result<Option<String>> {
    match fleet.member(&machine_id) {
        Some(machine) => direct::folder_branch(&fleet, machine, &path).await,
        None => Ok(None),
    }
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
