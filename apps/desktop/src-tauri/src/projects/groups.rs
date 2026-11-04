//! Project groups: folders the sidebar and the Projects page list projects
//! under, in place of their org. The page matches projects to them.

use super::known;
use crate::{
    beep::Service,
    center::kept,
    center::view_cache::{View, ViewCache},
    error::Result,
    fleet::projects::{FleetState, desk_or_server},
};
use beep_proto::{DeleteProjectGroup, ListProjectGroups, ProjectGroup, SaveProjectGroup};
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

/// The user's groups, oldest first; `None` from a server too old to keep
/// them, when the page offers none.
#[tauri::command]
pub async fn project_groups(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Option<Vec<ProjectGroup>>> {
    let groups = match desk_or_server(&fleet, &beep).await {
        Ok(desk) => {
            let mut groups = desk.map(|desk| desk.groups()).unwrap_or_default();
            // The groups only the server has, until they are moved into the mesh.
            if beep.server().is_some() {
                match known(beep.call(ListProjectGroups {}).await) {
                    Ok(theirs) => {
                        let theirs: Vec<ProjectGroup> = theirs
                            .into_iter()
                            .flatten()
                            .filter(|group| !groups.iter().any(|ours| ours.id == group.id))
                            .collect();
                        groups.extend(theirs);
                    }
                    Err(e) => log::info!("the server's groups did not read: {e}"),
                }
            }
            Ok(Some(groups))
        }
        Err(e) => Err(e),
    };
    kept(&cache, View::Groups, groups)
}

/// Makes a group without `id`, else changes the group `id`.
#[tauri::command]
pub async fn project_group_save(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    id: Option<Uuid>,
    name: String,
    paths: Vec<String>,
) -> Result<ProjectGroup> {
    let desk = desk_or_server(&fleet, &beep).await?;
    // A group only the server has is changed there.
    if let Some(id) = id
        && desk.as_ref().is_none_or(|desk| !desk.has_group(id))
        && beep.server().is_some()
    {
        return beep
            .call(SaveProjectGroup {
                id: Some(id),
                name,
                paths,
            })
            .await;
    }
    match desk {
        Some(desk) => Ok(desk.save_group(id, &name, &paths).await?),
        None => beep.call(SaveProjectGroup { id, name, paths }).await,
    }
}

#[tauri::command]
pub async fn project_group_delete(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    id: Uuid,
) -> Result<()> {
    match desk_or_server(&fleet, &beep).await? {
        Some(desk) if desk.has_group(id) || beep.server().is_none() => {
            Ok(desk.delete_group(id).await?)
        }
        _ => beep.call(DeleteProjectGroup { id }).await,
    }
}
