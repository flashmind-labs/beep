//! The projects the sidebar, Home and the Projects page list: imported
//! rows, project agents, pins to the top, and removing one.

use crate::{
    beep::{Service, epoch_ms},
    center::view_cache::{View, ViewCache},
    center::{self, kept},
    error::Result,
    fleet::projects::{FleetState, desk, mirror_remove},
};
use beep_proto::{ImportedProject, ListProjects, ProjectAgent};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

/// One imported project as the window shows it.
#[derive(Debug, Serialize)]
pub struct ProjectRow {
    /// `org/repo`.
    name: String,
    url: String,
    machine_id: String,
    machine_name: String,
    folder: String,
    cloned: bool,
    /// In milliseconds since the Unix epoch.
    imported_at: u64,
}

impl From<ImportedProject> for ProjectRow {
    fn from(p: ImportedProject) -> Self {
        Self {
            name: p.project,
            url: p.url,
            machine_id: p.machine_id,
            machine_name: p.machine_name,
            folder: p.folder,
            cloned: p.cloned,
            imported_at: epoch_ms(p.imported_at),
        }
    }
}

/// Every project agent of the user's; `None` from a server too old to have
/// them, and the page offers no project chat.
#[tauri::command]
pub async fn center_projects(
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<Vec<ProjectAgent>>> {
    let agents = match beep.call(ListProjects {}).await {
        Ok(agents) => Ok(Some(agents)),
        Err(e) if center::is_unsupported(&e) => Ok(None),
        Err(e) => Err(e),
    };
    kept(&cache, View::Agents, agents)
}

/// The projects pinned to the top of the sidebar, oldest pin first; none
/// from a server too old to pin them.
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

/// Removes `project` (`org/repo`) from Beep: it is forgotten and no longer synced, and the
/// server, if there is one, drops its chat and tasks. No files are deleted on any machine.
#[tauri::command]
pub async fn remove_project(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project: String,
) -> Result<()> {
    desk(&fleet).await?.remove(&project).await?;
    mirror_remove(&beep, &project).await;
    Ok(())
}
