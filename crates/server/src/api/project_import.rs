//! Importing projects onto the caller's machines and browsing their folders (`crate::projects::imports`).

use super::{Reply, branch_work::add_import_worktrees};
use crate::{
    api::router::Cx,
    projects::imports,
    protocol::{
        ApiError, CopyProject, CreateProject, CreatedProject, FindImportFork, HostRepos,
        ImportFork, ImportPreview, ImportProject, ImportedProject, ListHostRepos,
        ListImportedProjects, ListMachineFolder, ListProjectForges, ListProjectMachines,
        MachineFolder, PreviewImport, ProjectCopy, ProjectForge, ProjectMachine, RemoveProject,
    },
};
use beep_projects::imports::ImportError;
use beep_projects::imports::flow::Import;

fn refusal(e: ImportError) -> ApiError {
    match e {
        ImportError::Internal => ApiError::Internal {},
        other => ApiError::bad_request(other.to_string()),
    }
}

pub(super) async fn copy_project(cx: Cx, request: CopyProject) -> Reply<ProjectCopy> {
    imports::copy_project(&cx.app, &cx.user, &request.project, &request.machine_id)
        .await
        .map_err(refusal)
}

pub(super) async fn import_project(cx: Cx, request: ImportProject) -> Reply<ImportedProject> {
    let job = Import {
        repo: &request.repo,
        machine_id: request.machine_id.as_deref(),
        folder: request.folder.as_deref(),
        parent: request.parent.as_deref(),
    };
    let imported = imports::import(&cx.app, &cx.user, job)
        .await
        .map_err(refusal)?;
    add_import_worktrees(&cx, &imported, &request.worktrees).await;
    Ok(imported)
}

pub(super) async fn list_project_forges(cx: Cx, _: ListProjectForges) -> Reply<Vec<ProjectForge>> {
    imports::project_forges(&cx.app, &cx.user)
        .await
        .map_err(refusal)
}

pub(super) async fn create_project(cx: Cx, request: CreateProject) -> Reply<CreatedProject> {
    imports::create_project(&cx.app, &cx.user, &request)
        .await
        .map_err(refusal)
}

pub(super) async fn preview_import(cx: Cx, request: PreviewImport) -> Reply<ImportPreview> {
    imports::preview(&cx.app, &cx.user, &request.repo)
        .await
        .map_err(refusal)
}

pub(super) async fn find_import_fork(cx: Cx, request: FindImportFork) -> Reply<ImportFork> {
    imports::find_fork(&cx.app, &cx.user, &request.repo)
        .await
        .map_err(refusal)
}

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
    imports::project_machines(&cx.app, &cx.user, &request.project)
        .await?
        .ok_or(ApiError::NotFound {})
}

/// The caller's repositories to clone, per git host.
pub(super) async fn list_host_repos(cx: Cx, _: ListHostRepos) -> Reply<Vec<HostRepos>> {
    Ok(imports::host_repos(&cx.app, &cx.user).await?)
}
