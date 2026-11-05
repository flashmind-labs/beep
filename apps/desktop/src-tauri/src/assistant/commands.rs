//! The Assistant page's one command: whether chat runs on this Mac.

use crate::{beep::Service, error::Result, fleet::projects::FleetState};

use super::local;

/// Whether the chat is local: no server, and an OpenRouter key saved. The window shows the Chats
/// group and the chat pages on it, and the chat commands read this Mac's chats.
#[tauri::command]
pub async fn assistant_local(beep: Service<'_>, fleet: FleetState<'_>) -> Result<bool> {
    Ok(local::on(&beep, &fleet).await)
}
