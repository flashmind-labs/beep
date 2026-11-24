//! The New project sheet: the git hosts a repository can go on, and making
//! the project.

use super::ProjectRow;
use crate::{
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_proto::{
    CheckoutFailure, CreateProject, CreatedProject, NewRepository, ProjectFolder, ProjectForge,
};
use serde::Serialize;

/// What making a project did, as the sheet shows it.
#[derive(Debug, Serialize)]
pub struct CreatedRow {
    url: String,
    imported: Vec<ProjectRow>,
    failed: Vec<CheckoutFailure>,
}

impl From<CreatedProject> for CreatedRow {
    fn from(c: CreatedProject) -> Self {
        Self {
            url: c.url,
            imported: c.imported.into_iter().map(ProjectRow::from).collect(),
            failed: c.failed,
        }
    }
}

/// The git hosts the user has a token for.
#[tauri::command]
pub async fn project_forges(fleet: FleetState<'_>) -> Result<Option<Vec<ProjectForge>>> {
    Ok(Some(desk(&fleet).await?.forges().await?))
}

/// Makes the project `name`: on `repository` and cloned onto `machine_ids`, or
/// with no repository, `git init` on the one machine given; on each machine
/// under its folder in `folders`, else its projects folder.
#[tauri::command]
pub async fn create_project(
    fleet: FleetState<'_>,
    name: String,
    repository: Option<NewRepository>,
    machine_ids: Vec<String>,
    folders: Option<Vec<ProjectFolder>>,
) -> Result<CreatedRow> {
    let request = CreateProject {
        name,
        repository,
        machine_ids,
        folders: folders.unwrap_or_default(),
    };
    Ok(desk(&fleet).await?.create(&request).await?.into())
}
