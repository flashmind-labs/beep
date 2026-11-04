//! The model view: the account's model and thinking, Auto, and whether
//! agents are on. Changing them is for admins.

use crate::{beep::Service, error::Result};
use beep_proto::{
    Agents, GetAgents, GetModels, Models, ReasoningLevel, SetAgents, SetAutoModel, SetModel,
    SetThinking,
};

/// Whether agents are on, so the model view can say what Auto does. Admins only.
#[tauri::command]
pub async fn get_agents(beep: Service<'_>) -> Result<Agents> {
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
pub async fn get_models(beep: Service<'_>) -> Result<Models> {
    beep.call(GetModels {}).await
}

#[tauri::command]
pub async fn set_model(beep: Service<'_>, id: String) -> Result<Models> {
    beep.call(SetModel { id }).await
}

#[tauri::command]
pub async fn set_thinking(beep: Service<'_>, level: ReasoningLevel) -> Result<Models> {
    beep.call(SetThinking { level }).await
}

#[tauri::command]
pub async fn set_auto_model(beep: Service<'_>, enabled: bool) -> Result<Models> {
    beep.call(SetAutoModel { enabled }).await
}
