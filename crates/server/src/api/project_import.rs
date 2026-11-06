//! Importing projects onto the caller's machines and browsing their folders. The server reaches
//! no machine: a desktop imports over its mesh; the records stay listed here.

use super::{Reply, own_nodes::retired};
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

pub(super) async fn copy_project(_: Cx, _: CopyProject) -> Reply<ProjectCopy> {
    retired()
}

pub(super) async fn import_project(_: Cx, _: ImportProject) -> Reply<ImportedProject> {
    retired()
}

pub(super) async fn list_project_forges(_: Cx, _: ListProjectForges) -> Reply<Vec<ProjectForge>> {
    Ok(Vec::new())
}

pub(super) async fn create_project(_: Cx, _: CreateProject) -> Reply<CreatedProject> {
    retired()
}

pub(super) async fn preview_import(_: Cx, _: PreviewImport) -> Reply<ImportPreview> {
    retired()
}

pub(super) async fn find_import_fork(_: Cx, _: FindImportFork) -> Reply<ImportFork> {
    retired()
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

pub(super) async fn list_machine_folder(_: Cx, _: ListMachineFolder) -> Reply<MachineFolder> {
    retired()
}

pub(super) async fn list_project_machines(
    cx: Cx,
    request: ListProjectMachines,
) -> Reply<Vec<ProjectMachine>> {
    imports::project_machines(&cx.user, &request.project)
        .await?
        .ok_or(ApiError::NotFound {})
}

/// The repositories were read with a machine's `gh` and `teacli`; none is reached now.
pub(super) async fn list_host_repos(_: Cx, _: ListHostRepos) -> Reply<Vec<HostRepos>> {
    Ok(Vec::new())
}
