//! Newer `gh`, `teacli` and `flsh` on the account's machines, and installing them.

use crate::{
    beep::Service,
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, files as direct},
};
use beep_proto::{ToolUpdate, UpdateMachineTool, node::tool_update::Tool};
use futures::{StreamExt, stream};
use std::{sync::Arc, time::Duration};
use tauri::State;

/// How long a machine has to download and install a release.
const UPDATE_LIMIT: Duration = Duration::from_secs(11 * 60);

/// The updates found by each connected node.
#[tauri::command]
pub async fn tool_updates(fleet: State<'_, Arc<Fleet>>) -> Result<Vec<ToolUpdate>> {
    let machines = fleet.status().machines();
    let fleet = fleet.inner().as_ref();
    let updates = stream::iter(machines.into_iter().filter(|machine| machine.online))
        .map(|machine| async move {
            let Some(id) = fleet.member(&machine.id) else {
                return Vec::new();
            };
            let Ok(client) = fleet.client(id).await else {
                return Vec::new();
            };
            let Ok(updates) = beep_mesh::machine::tool_updates(&client).await else {
                return Vec::new();
            };
            updates
                .into_iter()
                .map(|update| ToolUpdate {
                    machine_id: machine.id.clone(),
                    tool: update.tool.to_string(),
                    version: update.version,
                    latest: update.latest,
                })
                .collect::<Vec<_>>()
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
    let mut updates: Vec<_> = updates.into_iter().flatten().collect();
    updates.sort_by(|a, b| (&a.machine_id, &a.tool).cmp(&(&b.machine_id, &b.tool)));
    Ok(updates)
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
    let client = fleet.client(machine).await?;
    let latest = beep_mesh::machine::tool_updates(&client)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("Could not check {named}: {e:#}")))?
        .into_iter()
        .find(|update| update.tool == named)
        .map(|update| update.latest)
        .ok_or_else(|| DesktopError::BadRequest(format!("No update found for {named}.")))?;
    direct::update_tool(&fleet, machine, named, &latest).await?;
    Ok(latest)
}
