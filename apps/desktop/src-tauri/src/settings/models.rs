//! The model view: the account's model and thinking, Auto, and whether
//! agents are on. Changing them is for admins.

use crate::{
    assistant::local::{self, Change},
    beep::Service,
    error::Result,
    fleet::projects::FleetState,
};
use beep_proto::{
    Agents, GetAgents, GetModels, Models, ReasoningLevel, SetAgents, SetAutoModel, SetModel,
    SetThinking,
};
use tauri::AppHandle;

/// Whether agents are on, so the model view can say what Auto does. Admins only.
#[tauri::command]
pub async fn get_agents(beep: Service<'_>, fleet: FleetState<'_>) -> Result<Agents> {
    // Agents are the server's; a chat kept on this Mac has none.
    if local::on(&beep, &fleet).await {
        return Ok(Agents {
            enabled: false,
            agents: Vec::new(),
        });
    }
    beep.call(GetAgents {}).await
}

/// Turns agents on or off for everyone. Admins only.
#[tauri::command]
pub async fn set_agents(beep: Service<'_>, enabled: bool) -> Result<Agents> {
    beep.call(SetAgents { enabled }).await
}

/// The caller's model settings. `choices` is empty unless they are an
/// admin, and the bar shows the picker only when it is not.
#[tauri::command]
pub async fn get_models(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
) -> Result<Models> {
    if local::on(&beep, &fleet).await {
        return local::models(&app, &fleet, Change::Nothing).await;
    }
    beep.call(GetModels {}).await
}

#[tauri::command]
pub async fn set_model(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    id: String,
) -> Result<Models> {
    if local::on(&beep, &fleet).await {
        return local::models(&app, &fleet, Change::Model(id)).await;
    }
    beep.call(SetModel { id }).await
}

#[tauri::command]
pub async fn set_thinking(
    app: AppHandle,
    beep: Service<'_>,
    fleet: FleetState<'_>,
    level: ReasoningLevel,
) -> Result<Models> {
    if local::on(&beep, &fleet).await {
        return local::models(&app, &fleet, Change::Effort(local::effort(level))).await;
    }
    beep.call(SetThinking { level }).await
}

#[tauri::command]
pub async fn set_auto_model(beep: Service<'_>, enabled: bool) -> Result<Models> {
    beep.call(SetAutoModel { enabled }).await
}
