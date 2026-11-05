//! The one owner of the MCP connections, on top of flashmind's `McpRegistry` (which speaks the
//! protocol over stdio and HTTP). A server is started the first time a chat needs it, kept for
//! later chats, and ended when it is removed or the app quits. A server that fails to start leaves
//! an error on its row and gives the chat no tools; it never fails the chat.
//!
//! The registry's own permission paths (`skips_permissions`, restricted tools) belong to its tool
//! wrappers, which this does not use: `call_tool` runs what it is told, and the chat's approval
//! card decides. The server list stays in our file (`config`), so the registry gets an in-memory
//! provider that stores nothing.

use super::{
    McpError,
    config::{McpServer, Transport},
};
use anyhow::Result;
use async_trait::async_trait;
use flashmind_tools::mcp::{
    McpConfigProvider, McpContent, McpRegistry, McpServerConfig, McpToolCallResult, McpToolDef,
};
use futures::future::join_all;
use secrecy::ExposeSecret;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
    time::Duration,
};
use tokio::time::timeout;

/// Tools offered to the model over all servers.
const MAX_TOOLS: usize = 64;
/// Characters kept of a tool's description.
const DESCRIPTION_CHARS: usize = 400;
/// Longest name an OpenAI-style tool may have.
const MAX_NAME: usize = 64;
/// Starting a server and listing its tools.
const OPEN_LIMIT: Duration = Duration::from_secs(30);
/// One tool call.
const CALL_LIMIT: Duration = Duration::from_secs(60);
/// Largest input schema passed on to the model, serialized.
const MAX_SCHEMA: usize = 16 * 1024;

/// A tool of a server, as it is offered to the model. The registry does not pass on a tool's
/// `readOnlyHint`, so every tool asks first.
#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    /// What the model calls it: `mcp_<server>_<tool>`.
    pub name: String,
    pub server: String,
    /// The server's own name for it.
    pub tool: String,
    pub description: String,
    pub schema: Value,
}

/// How a listed server is doing, as its row shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum McpStatus {
    /// Not started in this run of the app.
    Idle,
    Ready {
        tools: usize,
    },
    Failed {
        error: String,
    },
}

/// What a tool call came to.
pub(crate) struct Called {
    pub text: String,
    pub ok: bool,
}

/// The registry wants somewhere to save configs and OAuth credentials. The list is ours and
/// there is no OAuth here, so this keeps nothing.
struct NoStore;

#[async_trait]
impl McpConfigProvider for NoStore {
    async fn list_configs(&self) -> Result<Vec<McpServerConfig>> {
        Ok(Vec::new())
    }

    async fn save_config(&self, _: &McpServerConfig) -> Result<()> {
        Ok(())
    }

    async fn delete_config(&self, _: &str) -> Result<()> {
        Ok(())
    }

    async fn save_credentials(&self, _: &str, _: &Value) -> Result<()> {
        Ok(())
    }

    async fn load_credentials(&self, _: &str) -> Result<Option<Value>> {
        Ok(None)
    }
}

pub struct McpHub {
    registry: McpRegistry,
    /// The servers the registry knows, by name.
    added: Mutex<HashSet<String>>,
    status: Mutex<HashMap<String, McpStatus>>,
}

impl Default for McpHub {
    fn default() -> Self {
        // The registry's HTTP client takes no TLS provider of its own and panics without one
        // installed. One already installed by the app is kept: this then fails, which is fine.
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        Self {
            registry: McpRegistry::new(std::sync::Arc::new(NoStore), None),
            added: Mutex::default(),
            status: Mutex::default(),
        }
    }
}

fn clean(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `mcp_<server>_<tool>`, which only has characters a tool name may have. A server name has no
/// underscore, so the two parts can be told apart again.
fn wire_name(server: &str, tool: &str) -> String {
    let mut name = format!("mcp_{server}_{}", clean(tool));
    name.truncate(MAX_NAME);
    name
}

/// The registry's configuration for `server`. It holds secrets: never log it.
fn registry_config(server: &McpServer) -> McpServerConfig {
    let base = McpServerConfig {
        name: server.name.clone(),
        ..McpServerConfig::default()
    };
    match &server.transport {
        Transport::Stdio { command, args, env } => McpServerConfig {
            command: Some(command.clone()),
            args: args.clone(),
            env: env
                .iter()
                .map(|v| (v.name.clone(), v.value.expose_secret().to_owned()))
                .collect(),
            ..base
        },
        Transport::Http { url, token } => McpServerConfig {
            url: Some(url.to_string()),
            bearer_token: token.as_ref().map(|t| t.expose_secret().to_owned()),
            ..base
        },
    }
}

/// An error's text with the server's address taken out: it may carry a key in its query.
fn failure(server: &McpServer, e: &anyhow::Error) -> McpError {
    let mut text = format!("{e:#}");
    if let Transport::Http { url, .. } = &server.transport {
        text = text.replace(url.as_str(), &server.summary().detail);
    }
    McpError::Failed(text)
}

/// The text of a call result; a file is named, not shown.
fn result_text(result: &McpToolCallResult) -> String {
    let parts: Vec<&str> = result
        .content
        .iter()
        .map(|content| match content {
            McpContent::Blob(_) => "[file not shown]",
            McpContent::Text(text) | McpContent::Other(text) => text.as_str(),
        })
        .collect();
    parts.join("\n")
}

impl McpHub {
    fn set_status(&self, name: &str, status: McpStatus) {
        let mut all = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        all.insert(name.to_owned(), status);
    }

    /// What the page shows for `name`.
    pub fn status(&self, name: &str) -> McpStatus {
        let all = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        all.get(name).cloned().unwrap_or(McpStatus::Idle)
    }

    /// The tools of `server`, starting it if it is not running.
    async fn list(&self, server: &McpServer) -> Result<Vec<McpToolDef>, McpError> {
        let first = self
            .added
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(server.name.clone());
        if first {
            let added = self.registry.add(registry_config(server)).await;
            added.map_err(|e| failure(server, &e))?;
        }
        let listed = timeout(OPEN_LIMIT, self.registry.list_tools(&server.name))
            .await
            .map_err(|_| McpError::Timeout)?;
        match listed {
            Ok(tools) => Ok(tools),
            Err(_) => {
                // A connection that went stale: one fresh start before giving up.
                let again = timeout(OPEN_LIMIT, self.registry.reconnect(&server.name))
                    .await
                    .map_err(|_| McpError::Timeout)?;
                again.map_err(|e| failure(server, &e))
            }
        }
    }

    /// The tools of `servers`, starting the ones not running. A server that fails adds nothing.
    pub async fn tools(&self, servers: &[McpServer]) -> Vec<McpTool> {
        let listed = join_all(servers.iter().map(|server| async move {
            let result = self.list(server).await;
            match &result {
                Ok(defs) => self.set_status(&server.name, McpStatus::Ready { tools: defs.len() }),
                Err(e) => self.set_status(
                    &server.name,
                    McpStatus::Failed {
                        error: e.to_string(),
                    },
                ),
            }
            (server.name.clone(), result.unwrap_or_default())
        }))
        .await;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (server, defs) in listed {
            for def in defs {
                let name = wire_name(&server, &def.name);
                if out.len() >= MAX_TOOLS || !seen.insert(name.clone()) {
                    continue;
                }
                let schema = Some(def.input_schema)
                    .filter(|s| s.is_object() && s.to_string().len() <= MAX_SCHEMA)
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
                out.push(McpTool {
                    name,
                    server: server.clone(),
                    tool: def.name,
                    description: def
                        .description
                        .unwrap_or_default()
                        .chars()
                        .take(DESCRIPTION_CHARS)
                        .collect(),
                    schema,
                });
            }
        }
        out
    }

    /// Calls `tool`. The registry starts the server again once if its connection went stale.
    pub(super) async fn call(&self, tool: &McpTool, arguments: Value) -> Result<Called, McpError> {
        let call = self.registry.call_tool(&tool.server, &tool.tool, arguments);
        let result = timeout(CALL_LIMIT, call)
            .await
            .map_err(|_| McpError::Timeout)?
            .map_err(|e| McpError::Failed(format!("{e:#}")))?;
        Ok(Called {
            text: result_text(&result),
            ok: !result.is_error,
        })
    }

    /// Starts `server` again and says how it went: the Check button.
    pub async fn check(&self, server: &McpServer) -> McpStatus {
        self.forget(&server.name).await;
        self.tools(std::slice::from_ref(server)).await;
        self.status(&server.name)
    }

    /// Ends `name`'s process and drops what is known of it, as it is removed or checked again.
    pub async fn forget(&self, name: &str) {
        self.added
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
        if let Err(e) = self.registry.remove(name).await {
            log::warn!("removing MCP server {name}: {e}");
        }
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
    }

    /// Ends every process, as the app quits.
    pub async fn shutdown(&self) {
        self.registry.shutdown_all().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_only_use_what_a_name_may_hold() {
        assert_eq!(wire_name("files", "read.file"), "mcp_files_read_file");
        let long = wire_name("files", &"x".repeat(100));
        assert_eq!(long.len(), MAX_NAME);
        assert!(long.starts_with("mcp_files_"));
    }
}
