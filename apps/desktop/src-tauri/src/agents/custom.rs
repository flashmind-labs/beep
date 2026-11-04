//! The Custom agents list of a project page: the named agents the coding
//! agent harnesses define in the project's folder on one machine, and the
//! user's own there.

use crate::{beep::Service, error::Result};
use beep_proto::{CustomAgent, ListCustomAgents};
use serde::Serialize;

/// One custom agent, for the page.
#[derive(Debug, Serialize)]
pub struct CustomAgentRow {
    name: String,
    /// `claude`, `opencode`, `codex`, `pi` or `flsh`.
    harness: &'static str,
    /// `project` or `user`.
    scope: &'static str,
    description: String,
    model: Option<String>,
    tools: Vec<String>,
    color: Option<String>,
    path: String,
    prompt: String,
    prompt_cut: bool,
    error: Option<String>,
}

impl From<CustomAgent> for CustomAgentRow {
    fn from(a: CustomAgent) -> Self {
        Self {
            name: a.name,
            harness: a.harness.as_str(),
            scope: a.scope.as_str(),
            description: a.description,
            model: a.model,
            tools: a.tools,
            color: a.color,
            path: a.path,
            prompt: a.prompt,
            prompt_cut: a.prompt_cut,
            error: a.error,
        }
    }
}

#[tauri::command]
pub async fn custom_agents(
    beep: Service<'_>,
    machine_id: String,
    folder: String,
) -> Result<Vec<CustomAgentRow>> {
    let agents = beep.call(ListCustomAgents { machine_id, folder }).await?;
    Ok(agents.into_iter().map(CustomAgentRow::from).collect())
}
