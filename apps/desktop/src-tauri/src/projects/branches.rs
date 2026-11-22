//! Branches with work before an import, and giving a branch a worktree.

use crate::{
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        files::{self as direct, FALLBACK_DEFAULT, Place},
        projects::{FleetState, desk},
    },
};
use beep_proto::{ImportFork, ImportPreview, ProjectBranches};

/// Where `project` has a checkout on a machine of the mesh that is online, nearest first. With
/// none online, or a project the mesh does not record, that is an error.
pub(crate) fn placed(fleet: &Fleet, project: &str) -> Result<Vec<Place>> {
    let online = fleet.online();
    let mesh = fleet
        .mesh()
        .filter(|mesh| beep_fleet::has_project(mesh, project))
        .ok_or_else(|| DesktopError::BadRequest("The project is not in the mesh.".into()))?;
    let mut recorded: Vec<Place> = beep_fleet::places(&mesh, project)
        .into_iter()
        .filter(|(machine, _)| online.contains(machine))
        .map(|(machine, folder)| Place { machine, folder })
        .collect();
    beep_fleet::nearest_first(&mesh, &mut recorded, |place| place.machine);

    if recorded.is_empty() {
        return Err(DesktopError::BadRequest(
            "No machine with a checkout of the project is online.".into(),
        ));
    }

    Ok(recorded)
}

/// What importing `repo` would find.
#[tauri::command]
pub async fn preview_import(fleet: FleetState<'_>, repo: String) -> Result<Option<ImportPreview>> {
    Ok(Some(desk(&fleet).await?.preview(&repo).await?))
}

/// The user's fork that importing `repo` would clone.
#[tauri::command]
pub async fn find_import_fork(fleet: FleetState<'_>, repo: String) -> Result<Option<ImportFork>> {
    Ok(Some(desk(&fleet).await?.find_fork(&repo).await?))
}

/// `project`'s branches, read at its nearest online checkout on a machine of the mesh.
pub(crate) async fn read_branches(fleet: &Fleet, project: String) -> Result<ProjectBranches> {
    // `placed` lists one at least.
    let Some(place) = placed(fleet, &project)?.into_iter().next() else {
        return Err(DesktopError::BadRequest(
            "No machine with a checkout of the project is online.".into(),
        ));
    };
    let (default_branch, branches) = direct::branches(fleet, &place, FALLBACK_DEFAULT).await?;
    Ok(ProjectBranches {
        machine_id: place.machine.to_string(),
        machine_name: fleet.name_of(place.machine),
        default_branch,
        branches,
    })
}

/// `project`'s branches.
#[tauri::command]
pub async fn project_branches(
    fleet: Machines<'_>,
    project: String,
) -> Result<Option<ProjectBranches>> {
    Ok(Some(read_branches(&fleet, project).await?))
}
