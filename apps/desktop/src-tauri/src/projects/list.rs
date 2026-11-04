//! The projects the sidebar, Home and the Projects page list: imported
//! rows, project agents, pins to the top, and removing one.

use super::{known, needs_update};
use crate::{
    beep::{Service, epoch_ms},
    center::view_cache::{View, ViewCache},
    center::{self, kept},
    error::Result,
};
use beep_proto::{
    ImportedProject, ListProjects, ListProjectsOnTop, PinProjectToTop, ProjectAgent, RemoveProject,
};
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
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Vec<String>> {
    let names = known(beep.call(ListProjectsOnTop {}).await).map(Option::unwrap_or_default);
    kept(&cache, View::Top, names)
}

/// Pins `project` (`org/repo`) to the top of the sidebar, or unpins it.
/// Not `PinProject`, which keeps a project off sync.
#[tauri::command]
pub async fn pin_project_to_top(beep: Service<'_>, project: String, pinned: bool) -> Result<()> {
    beep.call(PinProjectToTop { project, pinned })
        .await
        .map_err(|e| needs_update(e, "before projects can be pinned"))
}

/// Removes `project` (`org/repo`) from Beep: the server forgets it, stops
/// syncing it and its workers, and deletes its chat and tasks. No files are
/// deleted on any machine.
#[tauri::command]
pub async fn remove_project(beep: Service<'_>, project: String) -> Result<()> {
    beep.call(RemoveProject { project })
        .await
        .map_err(|e| needs_update(e, "before projects can be removed"))
}
