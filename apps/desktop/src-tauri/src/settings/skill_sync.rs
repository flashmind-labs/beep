//! The mesh-wide switches: project and coding agent skill sync, and the
//! Docker containers sessions list.

use crate::{
    error::Result,
    fleet::projects::{FleetState, desk},
};

#[tauri::command]
pub async fn skill_sync_get(fleet: FleetState<'_>) -> Result<bool> {
    Ok(desk(&fleet).await?.skill_sync_enabled())
}

#[tauri::command]
pub async fn skill_sync_set(fleet: FleetState<'_>, enabled: bool) -> Result<()> {
    desk(&fleet).await?.set_skill_sync_enabled(enabled).await?;
    Ok(())
}

#[tauri::command]
pub async fn project_sync_get(fleet: FleetState<'_>) -> Result<bool> {
    Ok(desk(&fleet).await?.project_sync_enabled())
}

#[tauri::command]
pub async fn project_sync_set(fleet: FleetState<'_>, enabled: bool) -> Result<()> {
    desk(&fleet)
        .await?
        .set_project_sync_enabled(enabled)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn docker_containers_get(fleet: FleetState<'_>) -> Result<bool> {
    Ok(desk(&fleet).await?.docker_containers())
}

#[tauri::command]
pub async fn docker_containers_set(fleet: FleetState<'_>, enabled: bool) -> Result<()> {
    desk(&fleet).await?.set_docker_containers(enabled).await?;
    Ok(())
}
