//! The Add a project sheet: checkouts and host repositories to pick from,
//! the folder choice, and importing a project onto a machine.

use super::list::ProjectRow;
use crate::{
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::{
        Machines, files as direct,
        projects::{FleetState, desk},
    },
    machines::list::MachineLists,
};
use beep_fleet::ImportJob;
use beep_proto::{
    BranchPlace, FoundAgent, FoundPlace, FoundProject, GitHostKind, HostRepos, MachineFolder,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::State;

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
    session: FleetState<'_>,
    held: State<'_, MachineLists>,
) -> Result<Vec<FoundRow>> {
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
    rows: Vec<FoundRow>,
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
async fn grouped(session: &FleetState<'_>, held: &MachineLists) -> Vec<FoundRow> {
    let here = held.found.last();
    let found = match desk(session).await {
        Ok(desk) => desk.group_found(here),
        Err(_) => beep_projects::imports::found::group(here, &[], &[]),
    };
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
    repo: String,
    machine_id: Option<String>,
    folder: Option<String>,
    worktrees: Option<Vec<WorktreeArg>>,
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
