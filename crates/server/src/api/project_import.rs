//! Imported project records and machine placement. Removing records contacts no machine.

use super::Reply;
use crate::{
    api::router::Cx,
    projects::imports,
    protocol::{
        ApiError, ImportedProject, ListImportedProjects, ListProjectMachines, ProjectMachine,
        RemoveProject,
    },
};

pub(super) async fn list_imported_projects(
    cx: Cx,
    _: ListImportedProjects,
) -> Reply<Vec<ImportedProject>> {
    Ok(imports::imported(&cx.user).await?)
}

/// Removes one of the caller's projects; no machine's files are deleted.
pub(super) async fn remove_project(cx: Cx, request: RemoveProject) -> Reply<()> {
    let name = request.project.trim();
    if name.is_empty() {
        return Err(ApiError::NotFound {});
    }
    if imports::remove_project(&cx.app, &cx.user, name).await? {
        Ok(())
    } else {
        Err(ApiError::NotFound {})
    }
}

pub(super) async fn list_project_machines(
    cx: Cx,
    request: ListProjectMachines,
) -> Reply<Vec<ProjectMachine>> {
    imports::project_machines(&cx.user, &request.project)
        .await?
        .ok_or(ApiError::NotFound {})
}
