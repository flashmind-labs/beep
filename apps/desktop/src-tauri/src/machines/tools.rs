//! Newer `gh`, `teacli` and `flsh` on the account's machines, and installing them.

use crate::{
    beep::Service,
    error::{DesktopError, Result},
    fleet::{Machines, files as direct},
    projects::unknown_op,
};
use beep_proto::{ListToolUpdates, ToolUpdate, UpdateMachineTool, node::tool_update::Tool};
use std::time::Duration;

/// How long the server may take to look up the newest releases.
const LIST_LIMIT: Duration = Duration::from_secs(30);
/// How long a machine has to download and install a release.
const UPDATE_LIMIT: Duration = Duration::from_secs(11 * 60);

/// The tools behind their newest release; none without a server, or from one
/// without it.
#[tauri::command]
pub async fn tool_updates(beep: Service<'_>) -> Result<Vec<ToolUpdate>> {
    // The newest releases are looked up by the server.
    if beep.server().is_none() {
        return Ok(Vec::new());
    }
    match beep.call_within(ListToolUpdates {}, LIST_LIMIT).await {
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        listed => listed,
    }
}

/// Installs the newest `tool` on `machine_id`; gives the version installed.
#[tauri::command]
pub async fn update_machine_tool(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    tool: String,
) -> Result<String> {
    let Some(machine) = fleet.member(&machine_id) else {
        return beep
            .call_within(UpdateMachineTool { machine_id, tool }, UPDATE_LIMIT)
            .await;
    };
    let named: Tool = tool
        .parse()
        .map_err(|_| DesktopError::BadRequest(format!("Beep does not update {tool}.")))?;
    let latest = fleet.releases().latest(named).await.ok_or_else(|| {
        DesktopError::BadRequest(format!("Could not look up the newest {named}."))
    })?;
    direct::update_tool(&fleet, machine, named, &latest).await?;
    Ok(latest)
}
