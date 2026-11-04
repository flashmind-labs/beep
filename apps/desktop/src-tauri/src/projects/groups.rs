//! Project groups: folders the sidebar and the Projects page list projects
//! under, in place of their org. The page matches projects to them.

use crate::{
    center::kept,
    center::view_cache::{View, ViewCache},
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_proto::ProjectGroup;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// The user's groups, oldest first; `None` from a server too old to keep
/// them, when the page offers none.
#[tauri::command]
pub async fn project_groups(
    fleet: FleetState<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<Vec<ProjectGroup>>> {
    let groups = desk(&fleet).await.map(|desk| Some(desk.groups()));
    kept(&cache, View::Groups, groups)
}

/// Makes a group without `id`, else changes the group `id`.
#[tauri::command]
pub async fn project_group_save(
    fleet: FleetState<'_>,
    id: Option<Uuid>,
    name: String,
    paths: Vec<String>,
) -> Result<ProjectGroup> {
    Ok(desk(&fleet).await?.save_group(id, &name, &paths).await?)
}

#[tauri::command]
pub async fn project_group_delete(fleet: FleetState<'_>, id: Uuid) -> Result<()> {
    Ok(desk(&fleet).await?.delete_group(id).await?)
}
