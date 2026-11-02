//! Importing a project onto one of the caller's machines, listing the ones
//! imported, and browsing a machine's folders to pick where one goes (see
//! `crate::imports`).

use super::Reply;
use crate::{
    imports::{self, Import, ImportError},
    protocol::{
        ApiError, ImportProject, ImportedProject, ListImportedProjects, ListMachineFolder,
        ListProjectMachines, MachineFolder, ProjectMachine,
    },
    router::Cx,
};

fn refusal(e: ImportError) -> ApiError {
    match e {
        ImportError::Internal => ApiError::Internal {},
        other => ApiError::bad_request(other.to_string()),
    }
}

pub(super) async fn import_project(cx: Cx, request: ImportProject) -> Reply<ImportedProject> {
    let job = Import {
        repo: &request.repo,
        machine_id: request.machine_id.as_deref(),
        folder: request.folder.as_deref(),
    };
    imports::import(&cx.app, &cx.user, job)
        .await
        .map_err(refusal)
}

pub(super) async fn list_imported_projects(
    cx: Cx,
    _: ListImportedProjects,
) -> Reply<Vec<ImportedProject>> {
    Ok(imports::imported(&cx.user).await?)
}

pub(super) async fn list_machine_folder(
    cx: Cx,
    request: ListMachineFolder,
) -> Reply<MachineFolder> {
    imports::browse(
        &cx.app,
        &cx.user,
        &request.machine_id,
        request.path.as_deref(),
    )
    .await
    .map_err(refusal)
}

pub(super) async fn list_project_machines(
    cx: Cx,
    request: ListProjectMachines,
) -> Reply<Vec<ProjectMachine>> {
    admin(&cx)?;
    imports::project_machines(&cx.app, &cx.user, &request.project)
        .await?
        .ok_or(ApiError::NotFound {})
}
