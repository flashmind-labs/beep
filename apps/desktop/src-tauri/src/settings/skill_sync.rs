//! The mesh-wide project and coding agent skill sync switches.

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
pub async fn beep_sync_over_syncthing_get(fleet: FleetState<'_>) -> Result<bool> {
    Ok(desk(&fleet).await?.beep_sync_over_syncthing())
}

#[tauri::command]
pub async fn beep_sync_over_syncthing_set(fleet: FleetState<'_>, enabled: bool) -> Result<()> {
    desk(&fleet)
        .await?
        .set_beep_sync_over_syncthing(enabled)
        .await?;
    Ok(())
}
