//! The user's MCP servers. The registry is built on first use.

use super::User;
use crate::{
    app::UserNotice,
    apps::mcp::{StoreProvider, app_tools, check_redirect, display_name, lost_login, preset},
    protocol::Notice,
};
use anyhow::{Result, bail};
use flashmind::{tools::mcp::McpRegistry, types::Tool};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

impl User {
    /// Forgets a server whose login was rejected and tells the user to
    /// connect it again. The app then lists it as not connected.
    async fn drop_lost_mcp_login(&self, name: &str) {
        match self.remove_mcp(name).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::warn!("mcp: unable to forget a lost login; server={name} error={error:#}");
                return;
            }
        }
        let app = display_name(name);
        log::warn!("mcp: {name} rejected the saved login, asking to reconnect; user={self}");
        let notice = UserNotice {
            user: self.id().to_owned(),
            notice: Notice {
                title: format!("Reconnect {app}"),
                body: format!("Beep lost access to {app}. Tap to connect it again."),
                session_id: String::new(),
                app: Some(name.to_owned()),
                approval: None,
                changed: false,
                group: false,
            },
        };
        // Nobody listening just means the phone is offline; Apps still shows it.
        let _ = self.notifications.send(notice);
    }

    pub(super) async fn mcp_registry(&self) -> &McpRegistry {
        self.mcp
            .get_or_init(|| async {
                let provider = Arc::new(StoreProvider {
                    user: self.me.clone(),
                });
                let registry = McpRegistry::new(provider, None);
                registry.load_saved().await;
                registry
            })
            .await
    }

    /// Names of the servers the user has credentials for.
    pub async fn mcp_connected(&self) -> Result<HashSet<String>> {
        Ok(self
            .mcp_servers()
            .await?
            .into_iter()
            .filter(|row| row.credentials.is_some())
            .map(|row| row.name)
            .collect())
    }

    /// Agent tools for the servers the user has connected, one per remote
    /// tool Beep exposes.
    pub async fn mcp_tools(&self) -> Vec<Arc<dyn Tool>> {
        let connected = match self.mcp_connected().await {
            Ok(connected) if !connected.is_empty() => connected,
            Ok(_) => return Vec::new(),
            Err(error) => {
                log::warn!("mcp: unable to list servers: {error:#}");
                return Vec::new();
            }
        };
        let registry = self.mcp_registry().await;
        // The tools are made from `current_mcp_tools` on every run, so the
        // queued register and unregister ops are not needed.
        registry.drain_pending_ops();
        app_tools(registry, &connected)
    }

    /// Calls one tool on a connected server outside of any run and returns its
    /// result, parsed as JSON when the server answered with JSON text.
    pub async fn call_mcp(&self, server: &str, tool: &str, arguments: Value) -> Result<Value> {
        let registry = self.mcp_registry().await;
        let result = registry.call_tool(server, tool, arguments).await;
        registry.drain_pending_ops();
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if lost_login(&error) {
                    self.drop_lost_mcp_login(server).await;
                }
                return Err(error);
            }
        };
        let text: String = result
            .content
            .iter()
            .filter_map(|content| content.as_text())
            .collect();
        if result.is_error {
            // Error text says what went wrong; it is clipped in case it echoes content.
            let reason: String = text.chars().take(200).collect();
            bail!("{server} {tool} returned an error: {reason}");
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// Starts OAuth and returns the URL to open in the browser.
    pub async fn start_mcp_auth(&self, name: &str, redirect_uri: &str) -> Result<String> {
        let Some(config) = preset(name) else {
            bail!("unknown MCP server");
        };
        check_redirect(redirect_uri)?;
        let registry = self.mcp_registry().await;
        registry.add(config).await?;
        registry.start_auth(name, redirect_uri).await
    }

    /// Exchanges the code from the redirect, saves the tokens and connects.
    pub async fn finish_mcp_auth(
        &self,
        name: &str,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<()> {
        let registry = self.mcp_registry().await;
        registry.complete_auth(name, code, state, issuer).await
    }

    /// Disconnects and forgets the server and its tokens. Returns whether it existed.
    pub async fn remove_mcp(&self, name: &str) -> Result<bool> {
        let existed = self.mcp_connected().await?.contains(name);
        let registry = self.mcp_registry().await;
        if registry.get_config(name).await.is_some() {
            registry.remove(name).await?;
        } else {
            self.delete_mcp_server(name).await?;
        }
        registry.drain_pending_ops();
        Ok(existed)
    }

    /// Reconnects every connected server once. Connecting refreshes an
    /// expired access token, which also rotates and saves the refresh token.
    pub(crate) async fn keep_mcp_alive(&self) {
        let connected = match self.mcp_connected().await {
            Ok(connected) if !connected.is_empty() => connected,
            Ok(_) => return,
            Err(error) => {
                log::warn!("mcp: keep-alive unable to list servers: {error:#}");
                return;
            }
        };
        let registry = self.mcp_registry().await;
        for name in connected {
            if let Err(error) = registry.reconnect(&name).await {
                log::warn!("mcp: keep-alive reconnect failed; server={name} error={error:#}");
                if lost_login(&error) {
                    self.drop_lost_mcp_login(&name).await;
                }
            }
        }
        registry.drain_pending_ops();
    }
}
