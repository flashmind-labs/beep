//! The "Add a machine" sheet: a one-time setup token the machine claims
//! with `beep-node setup <token>`. While open, the sheet polls
//! `node_token_machine` to learn which machine claimed it. Or another
//! machine of the account installs beep-node over its own ssh.

use crate::{
    beep::{Service, epoch_ms},
    error::{DesktopError, Result},
    fleet::{Machines, files as direct},
    projects::needs_update,
};
use beep_proto::{
    ApiError, CreateNodeToken, GetMachineSearch, GetNodeToken, RevokeNodeToken, SetMachineSearch,
    setup::setup_command,
};
use serde::Serialize;

/// A token as the sheet shows it.
#[derive(Debug, Serialize)]
pub struct TokenView {
    token: String,
    /// `beep-node setup <token>`, to copy.
    command: String,
    /// In milliseconds since the Unix epoch.
    expires_at: u64,
}

#[tauri::command]
pub async fn create_node_token(beep: Service<'_>, name: Option<String>) -> Result<TokenView> {
    let made = beep
        .call(CreateNodeToken { name })
        .await
        .map_err(outdated)?;
    Ok(TokenView {
        command: setup_command(&made.token),
        token: made.token,
        expires_at: epoch_ms(made.expires_at),
    })
}

/// The `Machine::id` that claimed `token`, `None` while it is unused.
#[tauri::command]
pub async fn node_token_machine(beep: Service<'_>, token: String) -> Result<Option<String>> {
    beep.call(GetNodeToken { token }).await.map_err(outdated)
}

/// A token already used, expired or revoked is no error: it works no more
/// either way.
#[tauri::command]
pub async fn revoke_node_token(beep: Service<'_>, token: String) -> Result<()> {
    match beep.call(RevokeNodeToken { token }).await {
        Ok(()) | Err(DesktopError::Api(ApiError::NotFound {})) => Ok(()),
        Err(e) => Err(outdated(e)),
    }
}

/// Where `machine_id` looks for git checkouts, `~/` for home.
#[tauri::command]
pub async fn machine_search(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
) -> Result<String> {
    if let Some(machine) = fleet.member(&machine_id) {
        return direct::machine_search(&fleet, machine).await;
    }
    beep.call(GetMachineSearch { machine_id }).await
}

/// Has `machine_id` look for git checkouts in `search` and walk it again.
#[tauri::command]
pub async fn set_machine_search(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    search: String,
) -> Result<String> {
    if let Some(machine) = fleet.member(&machine_id) {
        return direct::set_machine_search(&fleet, machine, &search).await;
    }
    beep.call(SetMachineSearch { machine_id, search }).await
}

/// An old server's refusal, in words the sheet shows.
fn outdated(e: DesktopError) -> DesktopError {
    needs_update(e, "before machines can be added from here")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_server_reads_as_needing_an_update() {
        let old = DesktopError::Api(ApiError::bad_request("unknown op 180"));
        assert!(outdated(old).to_string().contains("needs an update"));
        let other = DesktopError::Api(ApiError::bad_request("name too long"));
        assert_eq!(outdated(other).to_string(), "bad request: name too long");
    }
}
