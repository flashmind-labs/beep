//! The Home assistant's harness, model and thinking level, the same on every machine.

use crate::{
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_proto::node::mesh_state::AssistantSettings;

#[tauri::command]
pub async fn assistant_settings_get(fleet: FleetState<'_>) -> Result<AssistantSettings> {
    Ok(desk(&fleet).await?.assistant())
}

#[tauri::command]
pub async fn assistant_settings_save(
    fleet: FleetState<'_>,
    settings: AssistantSettings,
) -> Result<()> {
    Ok(desk(&fleet).await?.save_assistant(&settings).await?)
}
