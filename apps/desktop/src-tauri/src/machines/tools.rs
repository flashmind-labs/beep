//! Newer `gh` and `teacli` on the account's machines, and installing them.

use crate::{beep::Service, error::Result, projects::unknown_op};
use beep_proto::{ListToolUpdates, ToolUpdate, UpdateMachineTool};
use std::time::Duration;

/// How long the server may take to look up the newest releases.
const LIST_LIMIT: Duration = Duration::from_secs(30);
/// How long a machine has to download and install a release.
const UPDATE_LIMIT: Duration = Duration::from_secs(11 * 60);

/// The tools behind their newest release; none from a server without it.
#[tauri::command]
pub async fn tool_updates(beep: Service<'_>) -> Result<Vec<ToolUpdate>> {
    match beep.call_within(ListToolUpdates {}, LIST_LIMIT).await {
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        listed => listed,
    }
}

/// Installs the newest `tool` on `machine_id`; gives the version installed.
#[tauri::command]
pub async fn update_machine_tool(
    beep: Service<'_>,
    machine_id: String,
    tool: String,
) -> Result<String> {
    beep.call_within(UpdateMachineTool { machine_id, tool }, UPDATE_LIMIT)
        .await
}
