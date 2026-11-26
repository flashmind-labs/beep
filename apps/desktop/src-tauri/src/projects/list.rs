//! The projects the sidebar, Home and the Projects page list: imported
//! rows, pins to the top, and removing one.

use crate::{
    error::Result,
    fleet::projects::{FleetState, desk},
    store::kept,
    store::view_cache::{View, ViewCache},
};
use beep_fleet::{RenameRepositoryTarget, RepositoryRename, RepositoryRenameResult};
use beep_proto::ImportedProject;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

/// One imported project as the window shows it.
#[derive(Debug, Serialize)]
pub struct ProjectRow {
    /// `org/repo`.
    name: String,
    display_name: Option<String>,
    url: String,
    machine_id: String,
    machine_name: String,
    folder: String,
    cloned: bool,
}

impl From<ImportedProject> for ProjectRow {
    fn from(p: ImportedProject) -> Self {
        Self {
            name: p.project,
            display_name: p.display_name,
            url: p.url,
            machine_id: p.machine_id,
            machine_name: p.machine_name,
            folder: p.folder,
            cloned: p.cloned,
        }
    }
}

/// The projects pinned to the top of the sidebar, oldest pin first.
#[tauri::command]
pub async fn projects_on_top(
    fleet: FleetState<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Vec<String>> {
    let names = desk(&fleet).await.map(|desk| desk.on_top());
    kept(&cache, View::Top, names)
}

/// Pins `project` (`org/repo`) to the top of the sidebar, or unpins it.
#[tauri::command]
pub async fn pin_project_to_top(
    fleet: FleetState<'_>,
    project: String,
    pinned: bool,
) -> Result<()> {
    Ok(desk(&fleet).await?.pin(&project, pinned).await?)
}

/// Removes `project` (`org/repo`) from Beep: it is forgotten and no longer synced. No files are
/// deleted on any machine.
#[tauri::command]
pub async fn remove_project(app: AppHandle, fleet: FleetState<'_>, project: String) -> Result<()> {
    let cache = app.state::<crate::palette::PaletteCache>();
    let desk = desk(&fleet).await?;
    // Record locations even if the menu has not read this new import yet.
    let read = cache.ticket();
    cache.project_list(&app, read, desk.imported().await);
    desk.remove(&project).await?;
    // Nothing read of it is kept.
    fleet.forget();
    cache.forget_project(&app, &project);
    Ok(())
}

/// Changes the visible name while preserving the project's mesh identity.
#[tauri::command]
pub async fn rename_project(
    fleet: FleetState<'_>,
    project: String,
    name: String,
    repositories: Vec<RepositoryRename>,
) -> Result<Vec<RepositoryRenameResult>> {
    let results = desk(&fleet)
        .await?
        .rename_project(&project, &name, &repositories)
        .await?;
    fleet.forget();
    Ok(results)
}

/// Lists repository copies that can be selected independently for renaming.
#[tauri::command]
pub async fn project_rename_repositories(
    fleet: FleetState<'_>,
    project: String,
) -> Result<Vec<RenameRepositoryTarget>> {
    Ok(desk(&fleet).await?.rename_repositories(&project).await?)
}
