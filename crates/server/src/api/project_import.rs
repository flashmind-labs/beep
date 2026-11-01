//! Importing a project onto one of the caller's machines, and listing the
//! ones imported (see `crate::project_import`).

use super::Reply;
use crate::{
    instance::Privilege,
    project_import::{self, ImportError},
    protocol::{ApiError, ImportProject, ImportedProject, ListImportedProjects},
    router::Cx,
};

fn admin(cx: &Cx) -> Result<(), ApiError> {
    if cx.app.allows(Privilege::Nodes, &cx.user) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized {})
    }
}

pub(super) async fn import_project(cx: Cx, request: ImportProject) -> Reply<ImportedProject> {
    admin(&cx)?;
    project_import::import(
        &cx.app,
        &cx.user,
        &request.repo,
        request.machine_id.as_deref(),
    )
    .await
    .map_err(|e| match e {
        ImportError::Internal => ApiError::Internal {},
        other => ApiError::bad_request(other.to_string()),
    })
}

pub(super) async fn list_imported_projects(
    cx: Cx,
    _: ListImportedProjects,
) -> Reply<Vec<ImportedProject>> {
    admin(&cx)?;
    Ok(project_import::imported(&cx.user).await?)
}
