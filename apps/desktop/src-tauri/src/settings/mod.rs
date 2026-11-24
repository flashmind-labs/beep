//! Settings: credentials, git host sign-ins and what syncs to the machines.

pub(crate) mod assistant;
pub(crate) mod cli_login;
pub(crate) mod credentials;
pub(crate) mod gh_sign_in;
pub(crate) mod skill_libraries;
pub(crate) mod skill_sync;

use crate::{
    error::Result,
    fleet::projects::{FleetState, desk},
};
use beep_proto::node::mesh_state::EditorSettings;

#[tauri::command]
pub async fn editor_settings_get(fleet: FleetState<'_>) -> Result<EditorSettings> {
    Ok(desk(&fleet).await?.editor())
}

#[tauri::command]
pub async fn editor_settings_save(
    fleet: FleetState<'_>,
    settings: EditorSettings,
) -> Result<()> {
    Ok(desk(&fleet).await?.save_editor(&settings).await?)
}
