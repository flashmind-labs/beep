//! The Assistant page's MCP servers: the list in a file of this Mac, and what each server is
//! doing. The tools of the listed servers go to the model in local chats (`local::send`). Nothing
//! here touches the server's own app connections or the hidden Apps and MCP pages. Environment
//! values and tokens go in and never come back out: a row says only that they are set.

use super::local::{bad, chat_dir};
use crate::error::{DesktopError, Result};
use beep_assistant::{
    McpHub, McpServer, McpStatus, ServerSummary, Transport, add_server, load_servers, remove_server,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, State};

/// A server as the page lists it.
#[derive(Serialize)]
pub struct McpRow {
    #[serde(flatten)]
    summary: ServerSummary,
    status: McpStatus,
}

/// A server the user adds. The command is one line and the environment lines of `NAME=value`.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewMcpServer {
    Command {
        name: String,
        command: String,
        env: String,
    },
    Url {
        name: String,
        url: String,
        token: String,
    },
}

type Hub<'a> = State<'a, Arc<McpHub>>;

async fn listed(app: &AppHandle) -> Result<Vec<McpServer>> {
    load_servers(&chat_dir(app)?).await.map_err(bad)
}

#[tauri::command]
pub async fn list_mcp_servers(app: AppHandle, hub: Hub<'_>) -> Result<Vec<McpRow>> {
    let servers = listed(&app).await?;
    Ok(servers
        .iter()
        .map(|server| McpRow {
            summary: server.summary(),
            status: hub.status(&server.name),
        })
        .collect())
}

/// Saves the server, then starts it once to list its tools, so its row shows how it went. A
/// server that does not start stays listed with its error.
#[tauri::command]
pub async fn add_mcp_server(app: AppHandle, hub: Hub<'_>, server: NewMcpServer) -> Result<()> {
    let (name, transport) = match server {
        NewMcpServer::Command { name, command, env } => (name, Transport::command(&command, &env)),
        NewMcpServer::Url { name, url, token } => (name, Transport::url(&url, &token)),
    };
    let server = McpServer::new(&name, transport.map_err(bad)?).map_err(bad)?;
    add_server(&chat_dir(&app)?, server.clone())
        .await
        .map_err(bad)?;
    hub.check(&server).await;
    Ok(())
}

#[tauri::command]
pub async fn remove_mcp_server(app: AppHandle, hub: Hub<'_>, name: String) -> Result<()> {
    remove_server(&chat_dir(&app)?, &name).await.map_err(bad)?;
    hub.forget(&name).await;
    Ok(())
}

/// Starts the server again and lists its tools.
#[tauri::command]
pub async fn check_mcp_server(app: AppHandle, hub: Hub<'_>, name: String) -> Result<()> {
    let servers = listed(&app).await?;
    let Some(server) = servers.iter().find(|server| server.name == name) else {
        return Err(DesktopError::BadRequest(
            "That server is not listed.".into(),
        ));
    };
    hub.check(server).await;
    Ok(())
}
