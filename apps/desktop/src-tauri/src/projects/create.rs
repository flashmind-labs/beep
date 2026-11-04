//! The New project sheet: the git hosts a repository can go on, and making
//! the project.

use super::{ProjectRow, import::IMPORT_TIMEOUT, known, needs_update};
use crate::{
    beep::Service,
    error::{DesktopError, Result},
};
use beep_proto::{
    ApiError, CheckoutFailure, CreateProject, CreatedProject, GitHostKind, ListProjectForges,
    NewRepository, ProjectForge,
};
use serde::Serialize;

/// A git host the sheet offers.
#[derive(Debug, Serialize)]
pub struct ForgeRow {
    host: String,
    github: bool,
    owners: Vec<String>,
    error: Option<String>,
}

impl From<ProjectForge> for ForgeRow {
    fn from(f: ProjectForge) -> Self {
        Self {
            github: matches!(f.kind, GitHostKind::GitHub {}),
            host: f.host,
            owners: f.owners,
            error: f.error,
        }
    }
}

/// What making a project did, as the sheet shows it.
#[derive(Debug, Serialize)]
pub struct CreatedRow {
    name: String,
    url: String,
    imported: Vec<ProjectRow>,
    failed: Vec<CheckoutFailure>,
}

impl From<CreatedProject> for CreatedRow {
    fn from(c: CreatedProject) -> Self {
        Self {
            name: c.project,
            url: c.url,
            imported: c.imported.into_iter().map(ProjectRow::from).collect(),
            failed: c.failed,
        }
    }
}

/// The git hosts the user has a token for; `None` from a server too old to
/// make projects.
#[tauri::command]
pub async fn project_forges(beep: Service<'_>) -> Result<Option<Vec<ForgeRow>>> {
    let forges = known(beep.call(ListProjectForges {}).await)?;
    Ok(forges.map(|forges| forges.into_iter().map(ForgeRow::from).collect()))
}

/// Makes the project `name`: on `repository` and cloned onto `machine_ids`, or
/// with no repository, `git init` on the one machine given.
#[tauri::command]
pub async fn create_project(
    beep: Service<'_>,
    name: String,
    repository: Option<NewRepository>,
    machine_ids: Vec<String>,
) -> Result<CreatedRow> {
    beep.call_within(
        CreateProject {
            name,
            repository,
            machine_ids,
        },
        IMPORT_TIMEOUT,
    )
    .await
    .map(CreatedRow::from)
    .map_err(|e| match needs_update(e, "before it creates projects") {
        DesktopError::Api(ApiError::BadRequest { message }) => DesktopError::BadRequest(message),
        other => other,
    })
}
