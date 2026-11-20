//! The Home assistant's harness, model and thinking level, the same on every machine, and the
//! Hermes Agent it can use instead.

use crate::{
    error::{DesktopError, Result},
    fleet::{
        Machines,
        projects::{FleetState, desk},
    },
};
use beep_fleet::HermesStatus;
use beep_proto::{ApiError, CredentialCheck, SecretText, node::mesh_state::AssistantSettings};
use serde::Serialize;

/// The saved settings, and the Hermes connection beside them.
#[derive(Debug, Serialize)]
pub struct AssistantView {
    #[serde(flatten)]
    settings: AssistantSettings,
    hermes: Option<HermesStatus>,
}

#[tauri::command]
pub async fn assistant_settings_get(fleet: FleetState<'_>) -> Result<AssistantView> {
    let desk = desk(&fleet).await?;
    Ok(AssistantView {
        settings: desk.assistant(),
        hermes: desk.hermes_status(),
    })
}

/// Saves the harness pick. The Hermes connection is kept: it changes only by connecting or
/// disconnecting.
#[tauri::command]
pub async fn assistant_settings_save(
    fleet: FleetState<'_>,
    settings: AssistantSettings,
) -> Result<()> {
    let desk = desk(&fleet).await?;
    let settings = AssistantSettings {
        hermes: desk.assistant().hermes,
        ..settings
    };
    Ok(desk.save_assistant(&settings).await?)
}

/// Checks the Hermes dashboard from `machine` and, when it takes `token`, uses it for the
/// assistant. The token is never logged.
#[tauri::command]
pub async fn hermes_connect(
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    machine: String,
    host: String,
    port: u16,
    token: String,
) -> Result<CredentialCheck> {
    let machine = machines
        .member(&machine)
        .ok_or(DesktopError::Api(ApiError::NotFound {}))?;
    let token = SecretText::new(token);
    Ok(desk(&fleet)
        .await?
        .connect_hermes(machine, &host, port, token)
        .await?)
}

#[tauri::command]
pub async fn hermes_disconnect(fleet: FleetState<'_>) -> Result<()> {
    Ok(desk(&fleet).await?.disconnect_hermes().await?)
}
