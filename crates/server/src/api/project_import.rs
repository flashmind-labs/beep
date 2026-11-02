//! Importing a project onto one of the caller's machines, listing the ones
//! imported, and browsing a machine's folders to pick where one goes (see
//! `crate::imports`).

use super::Reply;
use crate::{
    imports::{self, Import, ImportError},
    instance::Privilege,
    protocol::{
        ApiError, ImportProject, ImportedProject, ListImportedProjects, ListMachineFolder,
        MachineFolder,
    },
    router::Cx,
};

fn admin(cx: &Cx) -> Result<(), ApiError> {
    if cx.app.allows(Privilege::Nodes, &cx.user) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized {})
    }
}

fn refusal(e: ImportError) -> ApiError {
    match e {
        ImportError::Internal => ApiError::Internal {},
        other => ApiError::bad_request(other.to_string()),
    }
}

pub(super) async fn import_project(cx: Cx, request: ImportProject) -> Reply<ImportedProject> {
    admin(&cx)?;
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
    admin(&cx)?;
    Ok(imports::imported(&cx.user).await?)
}

pub(super) async fn list_machine_folder(
    cx: Cx,
    request: ListMachineFolder,
) -> Reply<MachineFolder> {
    admin(&cx)?;
    imports::browse(
        &cx.app,
        &cx.user,
        &request.machine_id,
        request.path.as_deref(),
    )
    .await
    .map_err(refusal)
}
