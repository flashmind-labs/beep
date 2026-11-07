//! The mesh-wide coding agent skill sync setting.

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
