//! Project groups: folders the sidebar and the Projects page list projects
//! under, in place of their org. The page matches projects to them.

use crate::{
    beep::Service,
    center::kept,
    center::view_cache::{View, ViewCache},
    error::{DesktopError, Result},
    projects::{known, needs_update},
};
use beep_proto::{DeleteProjectGroup, ListProjectGroups, ProjectGroup, SaveProjectGroup};
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// The user's groups, oldest first; `None` from a server too old to keep
/// them, when the page offers none.
#[tauri::command]
pub async fn project_groups(
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<Vec<ProjectGroup>>> {
    let groups = known(beep.call(ListProjectGroups {}).await);
    kept(&cache, View::Groups, groups)
}

/// Makes a group without `id`, else changes the group `id`.
#[tauri::command]
pub async fn project_group_save(
    beep: Service<'_>,
    id: Option<Uuid>,
    name: String,
    paths: Vec<String>,
) -> Result<ProjectGroup> {
    beep.call(SaveProjectGroup { id, name, paths })
        .await
        .map_err(too_old)
}

#[tauri::command]
pub async fn project_group_delete(beep: Service<'_>, id: Uuid) -> Result<()> {
    beep.call(DeleteProjectGroup { id }).await.map_err(too_old)
}

fn too_old(e: DesktopError) -> DesktopError {
    needs_update(e, "before projects can be grouped")
}
