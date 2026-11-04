//! Newer `gh`, `teacli` and `flsh` on the account's machines, and installing them.

use crate::{
    beep::Service,
    error::{DesktopError, Result},
    fleet::{Machines, files as direct},
    projects::unknown_op,
};
use beep_proto::{
    ListToolUpdates, ToolUpdate, UpdateMachineTool,
    node::tool_update::{Releases, Tool},
};
use reqwest::header::USER_AGENT;
use serde::Deserialize;
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
    let latest = latest_release(named).await?;
    direct::update_tool(&fleet, machine, named, &latest).await?;
    Ok(latest)
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

/// The newest release of `tool`, such as `2.103.3`.
async fn latest_release(tool: Tool) -> Result<String> {
    let lookup = |e: reqwest::Error| {
        DesktopError::BadRequest(format!("Could not look up the newest {tool}: {e}"))
    };
    let http = reqwest::Client::new();
    match tool.releases() {
        Releases::Channel(base) => {
            let text = http
                .get(format!("{base}/latest.txt"))
                .header(USER_AGENT, "beep-desktop")
                .send()
                .await
                .and_then(|reply| reply.error_for_status())
                .map_err(lookup)?
                .text()
                .await
                .map_err(lookup)?;
            Ok(text.trim().to_owned())
        }
        Releases::GitHub(repo) => {
            let release: Release = http
                .get(format!(
                    "https://api.github.com/repos/{repo}/releases/latest"
                ))
                .header(USER_AGENT, "beep-desktop")
                .send()
                .await
                .and_then(|reply| reply.error_for_status())
                .map_err(lookup)?
                .json()
                .await
                .map_err(lookup)?;
            let tag = release.tag_name;
            Ok(tag.strip_prefix('v').unwrap_or(&tag).to_owned())
        }
    }
}
