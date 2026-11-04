//! Importing projects onto the caller's machines and browsing their folders (`crate::imports`).

use super::{Reply, branch_work::add_import_worktrees};
use crate::{
    imports::{self, Import, ImportError},
    protocol::{
        ApiError, FindImportFork, HostRepos, ImportFork, ImportPreview, ImportProject,
        ImportedProject, ListHostRepos, ListImportedProjects, ListMachineFolder,
        ListProjectMachines, MachineFolder, PreviewImport, ProjectMachine, RemoveProject,
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
        parent: request.parent.as_deref(),
    };
    let imported = imports::import(&cx.app, &cx.user, job)
        .await
        .map_err(refusal)?;
    add_import_worktrees(&cx, &imported, &request.worktrees).await;
    Ok(imported)
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
