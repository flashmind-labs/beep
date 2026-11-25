//! Newer `gh`, `teacli`, `flsh` and coding agents on the account's
//! machines, and installing them.

use crate::{
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, ToolPush, agents, files as direct},
};
use beep_proto::node::{events::Topic, harness::harness, setup::Need, tool_update::Tool};
use iroh::EndpointId;

/// Installs the newest `tool` on `machine_id`; gives the version installed.
#[tauri::command]
pub async fn update_machine_tool(
    app: tauri::AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    tool: String,
) -> Result<String> {
    let machine = fleet.machine(&machine_id)?;
    let Ok(named) = tool.parse::<Tool>() else {
        return update_agent(&app, &fleet, machine, &tool).await;
    };
    let latest = match fleet.projections().tool_pushes.get(machine) {
        Some(ToolPush::Pushed(updates)) => updates
            .into_iter()
            .find(|update| update.tool == named)
            .map(|update| update.latest),
        // A node that does not push its list is asked.
        _ => {
            let client = fleet.client(machine).await?;
            beep_mesh::machine::tool_updates(&client)
                .await
                .map_err(|e| DesktopError::BadRequest(format!("Could not check {named}: {e:#}")))?
                .into_iter()
                .find(|update| update.tool == named)
                .map(|update| update.latest)
        }
    }
    .ok_or_else(|| DesktopError::BadRequest(format!("No update found for {named}.")))?;
    direct::update_tool(&fleet, machine, named, &latest).await?;
    fleet.projections().tool_updates.forget(&machine);
    fleet.projections().tool_pushes.installed(machine, named);
    beep_node::events::system().changed(Topic::new("tools"));
    Ok(latest)
}

/// Has `machine_id` install `need` (`git` or `node`) for its agents: `true`
/// once installed, `false` when the system finishes it in a window of its
/// own, such as the Xcode tools dialog on macOS.
#[tauri::command]
pub async fn machine_set_up(fleet: Machines<'_>, machine_id: String, need: Need) -> Result<bool> {
    let machine = fleet
        .member(&machine_id)
        .ok_or_else(|| DesktopError::BadRequest("The machine is not in the mesh.".into()))?;
    let client = fleet.client(machine).await?;
    beep_mesh::machine::set_up(&client, need)
        .await
        .map_err(|e| DesktopError::BadRequest(format!("Could not install {need}: {e:#}")))
}

/// Updates coding agent `agent` on `machine`; gives the version it reports
/// after.
async fn update_agent(
    app: &tauri::AppHandle,
    fleet: &Fleet,
    machine: EndpointId,
    agent: &str,
) -> Result<String> {
    if harness(agent).is_none() {
        return Err(DesktopError::BadRequest(format!(
            "Beep does not update {agent}."
        )));
    }

    let report = agents::update_harness(fleet, machine, agent).await?;
    agents::models_changed(app, fleet, machine);
    report
        .harnesses
        .into_iter()
        .find(|item| item.agent == agent)
        .and_then(|item| item.version)
        .ok_or_else(|| {
            DesktopError::BadRequest(format!("{agent} did not say its version after the update."))
        })
}
