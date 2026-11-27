//! The Add a project sheet: checkouts and host repositories to pick from,
//! the folder choice, and importing a project onto a machine.

use super::list::ProjectRow;
use crate::{
    error::{DesktopError, Result},
    fleet::{
        Machines, files as direct,
        projects::{FleetState, desk},
    },
    machines::list::MachineLists,
};
use beep_fleet::ImportJob;
use beep_proto::{BranchPlace, FoundProject, HostRepos, MachineFolder};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

/// The checkouts on the machines that are not projects yet, as each machine
/// named them last: at once, for `rescan_found` to bring up to date.
#[tauri::command]
pub async fn found_projects(
    session: FleetState<'_>,
    held: State<'_, MachineLists>,
) -> Result<Vec<FoundProject>> {
    Ok(grouped(&session, &held).await)
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
    rows: Vec<FoundProject>,
    /// In words, when the machine could not say what it has.
    problem: Option<String>,
}

/// Has `machine_id` walk its search folder again, quickly, and name its
/// checkouts; gives the found list with what it named.
#[tauri::command]
pub async fn rescan_found(
    fleet: Machines<'_>,
    session: FleetState<'_>,
    held: State<'_, MachineLists>,
    machine_id: String,
) -> Result<FoundAfter> {
    let machine = fleet.machine(&machine_id)?;
    let problem = match direct::found_on(&fleet, machine).await {
        Ok(found) => {
            held.found.keep(found);
            None
        }
        Err(why) => Some(why),
    };
    Ok(FoundAfter {
        rows: grouped(&session, &held).await,
        problem,
    })
}

/// What the machines named last, without what the mesh already has as a
/// project.
async fn grouped(session: &FleetState<'_>, held: &MachineLists) -> Vec<FoundProject> {
    let here = held.found.last();
    match desk(session).await {
        Ok(desk) => desk.group_found(here),
        Err(_) => beep_projects::imports::found::group(here, &[], &[]),
    }
}

/// The user's repositories on their git hosts, to clone.
#[tauri::command]
pub async fn host_repos(fleet: FleetState<'_>) -> Result<Vec<HostRepos>> {
    Ok(desk(&fleet).await?.host_repos().await?)
}

/// Shows a project's folder on this Mac in Finder.
#[tauri::command]
pub fn open_folder(path: PathBuf) -> Result<()> {
    if !openable(&path) {
        return Err(DesktopError::BadRequest(
            "That folder is not on this computer.".into(),
        ));
    }

    crate::platform::open(&path)?;
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
#[allow(clippy::too_many_arguments)]
pub async fn import_project(
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    repo: String,
    machine_id: Option<String>,
    folder: Option<String>,
    worktrees: Option<Vec<BranchPlace>>,
    parent: Option<String>,
    syncthing_files: Option<bool>,
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
    let desk = desk(&fleet).await?;
    let imported = desk
        .import(ImportJob {
            repo: &repo,
            machine_id: machine_id.as_deref(),
            folder: folder.as_deref(),
            parent: parent.as_deref(),
            worktrees: &worktrees,
        })
        .await?;
    if let Some(enabled) = syncthing_files {
        desk.project_syncthing_mode(&imported.project, Some(enabled))
            .await?;
        machines.projections().forget_syncthing(&imported.project);
    }

    Ok(imported.into())
}

/// One folder on `machine_id`, for completing a typed path: `path`, else
/// the machine's home.
#[tauri::command]
pub async fn list_machine_folder(
    fleet: Machines<'_>,
    machine_id: String,
    path: Option<String>,
) -> Result<MachineFolder> {
    let mut tree = direct::folder(&fleet, fleet.machine(&machine_id)?, path.as_deref(), 0).await?;
    Ok(tree.swap_remove(0))
}

/// A folder on `machine_id` for the folder picker, `path` else the
/// machine's home, first, then the folders two levels below it.
#[tauri::command]
pub async fn list_machine_tree(
    fleet: Machines<'_>,
    machine_id: String,
    path: Option<String>,
) -> Result<Vec<MachineFolder>> {
    direct::folder(&fleet, fleet.machine(&machine_id)?, path.as_deref(), 2).await
}

#[cfg(test)]
#[path = "../tests/projects_import.rs"]
mod tests;
