//! Pairing a phone: it is a mesh observer, so the first launch has no mesh and takes a
//! desktop's phone ticket. On a desktop the pairing commands do nothing.

use super::Machines;
use crate::error::Result;

/// Whether this phone belongs to a mesh. Waits for the mesh to be looked for on launch. A
/// desktop reports `true`: it needs no pairing.
#[tauri::command]
pub async fn mobile_paired(fleet: Machines<'_>) -> Result<bool> {
    #[cfg(mobile)]
    return Ok(fleet.paired().await);
    #[cfg(desktop)]
    {
        let _ = fleet;
        Ok(true)
    }
}

/// Pairs this phone with the desktop that made `ticket`, a phone pairing ticket from its
/// Add machine sheet, as `name` (this phone's model by default). The pairing is kept in the
/// app's data folder.
#[tauri::command]
pub async fn mobile_pair(fleet: Machines<'_>, ticket: String, name: Option<String>) -> Result<()> {
    #[cfg(mobile)]
    {
        let name = name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Phone".into());
        fleet.pair(ticket.trim(), &name).await
    }
    #[cfg(desktop)]
    {
        let _ = (fleet, ticket, name);
        Err(crate::error::DesktopError::BadRequest(
            "only a phone pairs with a ticket".into(),
        ))
    }
}

/// The phone moved to another network, as its webview heard: every machine is dialed again.
#[tauri::command]
pub async fn mobile_network_changed(fleet: Machines<'_>) -> Result<()> {
    fleet.reconnect().await;
    Ok(())
}
