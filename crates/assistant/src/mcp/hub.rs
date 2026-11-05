//! The one owner of the MCP connections. A server is started the first time a chat needs it, kept
//! for later chats, and ended when it is removed or the app quits. A server that fails to start
//! leaves an error on its row and gives the chat no tools; it never fails the chat.

use super::{
    McpError,
    config::McpServer,
    conn::{Called, Conn},
};
use crate::AsyncMutex;
use futures::future::join_all;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, PoisonError},
};

/// Tools offered to the model over all servers.
const MAX_TOOLS: usize = 64;
/// Characters kept of a tool's description.
const DESCRIPTION_CHARS: usize = 400;
/// Longest name an OpenAI-style tool may have.
const MAX_NAME: usize = 64;

/// A tool of a server, as it is offered to the model.
#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    /// What the model calls it: `mcp_<server>_<tool>`.
    pub name: String,
    pub server: String,
    /// The server's own name for it.
    pub tool: String,
    pub description: String,
    pub schema: Value,
    /// The server says it only reads: it runs without asking.
    pub read_only: bool,
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

struct Slot {
    server: McpServer,
    conn: AsyncMutex<Option<Arc<Conn>>>,
}

#[derive(Default)]
pub struct McpHub {
    slots: AsyncMutex<HashMap<String, Arc<Slot>>>,
    status: Mutex<HashMap<String, McpStatus>>,
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

    /// The slot of `server`, made on first use.
    async fn slot(&self, server: &McpServer) -> Arc<Slot> {
        let mut slots = self.slots.lock().await;
        slots
            .entry(server.name.clone())
            .or_insert_with(|| {
                Arc::new(Slot {
                    server: server.clone(),
                    conn: AsyncMutex::new(None),
                })
            })
            .clone()
    }

    /// The open connection of `slot`, started now if there is none.
    async fn connected(&self, slot: &Slot) -> Result<Arc<Conn>, McpError> {
        let mut held = slot.conn.lock().await;
        if let Some(conn) = held.as_ref() {
            return Ok(conn.clone());
        }
        match Conn::open(&slot.server).await {
            Ok(conn) => {
                self.set_status(
                    &slot.server.name,
                    McpStatus::Ready {
                        tools: conn.tools.len(),
                    },
                );
                let conn = Arc::new(conn);
                *held = Some(conn.clone());
                Ok(conn)
            }
            Err(e) => {
                self.set_status(
                    &slot.server.name,
                    McpStatus::Failed {
                        error: e.to_string(),
                    },
                );
                Err(e)
            }
        }
    }

    /// The tools of `servers`, starting the ones not running. A server that fails adds nothing.
    pub async fn tools(&self, servers: &[McpServer]) -> Vec<McpTool> {
        let opened = join_all(servers.iter().map(|server| async move {
            let slot = self.slot(server).await;
            self.connected(&slot)
                .await
                .ok()
                .map(|conn| (server.name.clone(), conn))
        }))
        .await;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (server, conn) in opened.into_iter().flatten() {
            for raw in &conn.tools {
                let name = wire_name(&server, &raw.name);
                if out.len() >= MAX_TOOLS || !seen.insert(name.clone()) {
                    continue;
                }
                out.push(McpTool {
                    name,
                    server: server.clone(),
                    tool: raw.name.clone(),
                    description: raw.description.chars().take(DESCRIPTION_CHARS).collect(),
                    schema: raw.schema.clone(),
                    read_only: raw.read_only,
                });
            }
        }
        out
    }

    /// Calls `tool`. A connection that has closed is dropped, so the next chat starts the server
    /// again.
    pub(super) async fn call(&self, tool: &McpTool, arguments: Value) -> Result<Called, McpError> {
        let slot = {
            let slots = self.slots.lock().await;
            slots.get(&tool.server).cloned().ok_or(McpError::Closed)?
        };
        let conn = self.connected(&slot).await?;
        let called = conn.call(&tool.tool, arguments).await;
        if matches!(called, Err(McpError::Closed)) {
            let mut held = slot.conn.lock().await;
            // Only the connection that failed: another call may have started a new one.
            if held.as_ref().is_some_and(|now| Arc::ptr_eq(now, &conn)) {
                conn.close();
                *held = None;
            }
            self.set_status(
                &tool.server,
                McpStatus::Failed {
                    error: McpError::Closed.to_string(),
                },
            );
        }
        called
    }

    /// Starts `server` again and says how it went: the Check button.
    pub async fn check(&self, server: &McpServer) -> McpStatus {
        self.forget(&server.name).await;
        self.tools(std::slice::from_ref(server)).await;
        self.status(&server.name)
    }

    /// Ends `name`'s process and drops what is known of it, as it is removed or checked again.
    pub async fn forget(&self, name: &str) {
        let slot = self.slots.lock().await.remove(name);
        if let Some(slot) = slot
            && let Some(conn) = slot.conn.lock().await.take()
        {
            conn.close();
        }
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
    }

    /// Ends every process, as the app quits.
    pub async fn shutdown(&self) {
        let slots: Vec<Arc<Slot>> = self
            .slots
            .lock()
            .await
            .drain()
            .map(|(_, slot)| slot)
            .collect();
        for slot in slots {
            // One still starting is a local of the task that starts it, which ends with the app.
            if let Ok(mut held) = slot.conn.try_lock()
                && let Some(conn) = held.take()
            {
                conn.close();
            }
        }
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
