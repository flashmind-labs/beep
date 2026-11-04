//! The user's MCP servers. The registry is built on first use.

mod custom;
mod fastmail;

use super::User;
use crate::{
    app::{Reach, UserNotice},
    apps::{
        custom_mcp::{
            CustomProvider, config_of, custom_tools, enabled_names, info, is_custom, ready,
        },
        mcp::{
            FASTMAIL, FastmailAccount, app_tools, display_name, is_fastmail, lost_login, preset,
        },
    },
    protocol::{McpServer, Notice},
    store::McpServerRow,
};
use anyhow::{Result, bail};
use flashmind::{
    tools::mcp::{McpRegistry, McpToolCallResult},
    types::Tool,
};
use reqwest::Url;
use serde_json::Value;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

/// How long a sign-in may take before its `state` is forgotten.
const FLOW_TTL: Duration = Duration::from_secs(10 * 60);

/// A sign-in to an added server under way.
pub(crate) struct Flow {
    server: String,
    started: Instant,
}

impl User {
    /// The saved credentials of the server `name`.
    pub(crate) async fn mcp_credentials(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .mcp_servers()
            .await?
            .into_iter()
            .find(|row| row.name == name)
            .and_then(|row| row.credentials))
    }

    /// Forgets a server whose login was rejected and tells the user to connect it again.
    pub(crate) async fn drop_lost_mcp_login(&self, name: &str) {
        if is_custom(name) {
            self.drop_lost_custom_login(name).await;
            return;
        }
        let address = self
            .fastmail_accounts()
            .await
            .ok()
            .and_then(|accounts| accounts.into_iter().find(|account| account.name == name))
            .and_then(|account| account.address);
        match self.remove_mcp(name).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::warn!("mcp: unable to forget a lost login; server={name} error={error:#}");
                return;
            }
        }
        let app = display_name(name);
        let which = match &address {
            Some(address) => format!("{app} ({address})"),
            None => app.to_owned(),
        };
        log::warn!("mcp: {name} rejected the saved login, asking to reconnect; user={self}");
        let notice = UserNotice {
            user: self.id().to_owned(),
            notice: Notice {
                title: format!("Reconnect {app}"),
                body: format!("Beep lost access to {which}. Tap to connect it again."),
                session_id: String::new(),
                // The app's slug, which the phone connects again.
                app: Some(if is_fastmail(name) { FASTMAIL } else { name }.to_owned()),
                approval: None,
                changed: false,
                group: false,
            },
            desktop: None,
            to: Reach::Everyone,
        };
        // Nobody listening just means the phone is offline; Apps still shows it.
        let _ = self.notifications.send(notice);
    }

    pub(super) async fn mcp_registry(&self) -> Arc<McpRegistry> {
        self.mcp.get(&self.me).await
    }

    /// The registry of servers the user added by URL, filled as calls need them.
    async fn custom_registry(&self) -> &McpRegistry {
        self.custom_mcp
            .get_or_init(|| async {
                let provider = Arc::new(CustomProvider {
                    user: self.me.clone(),
                });
                McpRegistry::new(provider, None)
            })
            .await
    }

    /// Slugs of the catalog apps the user has credentials for, not servers they added.
    pub async fn mcp_connected(&self) -> Result<HashSet<String>> {
        Ok(self
            .connected_servers()
            .await?
            .into_iter()
            .map(|name| {
                if is_fastmail(&name) {
                    FASTMAIL.to_owned()
                } else {
                    name
                }
            })
            .collect())
    }

    /// Catalog servers with credentials, one per Fastmail account.
    async fn connected_servers(&self) -> Result<Vec<String>> {
        Ok(self
            .mcp_servers()
            .await?
            .into_iter()
            .filter(|row| row.credentials.is_some() && preset(&row.name).is_some())
            .map(|row| row.name)
            .collect())
    }

    /// Agent tools for the user's connected servers, one per remote tool.
    pub async fn mcp_tools(&self) -> Vec<Arc<dyn Tool>> {
        let rows = match self.mcp_servers().await {
            Ok(rows) if !rows.is_empty() => rows,
            Ok(_) => return Vec::new(),
            Err(error) => {
                log::warn!("mcp: unable to list servers: {error:#}");
                return Vec::new();
            }
        };
        let accounts = fastmail_rows(&rows);
        let mut tools = Vec::new();
        if !accounts.is_empty() {
            if accounts.len() > 1 {
                // The agent picks accounts by address, so learn missing ones for the next run.
                self.learn_addresses_later(&accounts);
            }
            let registry = self.mcp_registry().await;
            // Tools are rebuilt from `current_mcp_tools` every run; the queued ops are not needed.
            registry.drain_pending_ops();
            tools = app_tools(&self.mcp, &registry, &accounts);
        }
        if rows.iter().any(ready) {
            let registry = self.custom_registry().await;
            registry.drain_pending_ops();
            tools.extend(custom_tools(registry, &self.me, self.id(), &rows));
        }
        tools
    }

    /// Calls one tool outside any run; JSON text in the result is parsed.
    pub async fn call_mcp(&self, server: &str, tool: &str, arguments: Value) -> Result<Value> {
        let result = self.call_mcp_content(server, tool, arguments).await?;
        let text: String = result
            .content
            .iter()
            .filter_map(|content| content.as_text())
            .collect();
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// Calls one tool outside any run, keeping any files it returns.
    pub(crate) async fn call_mcp_content(
        &self,
        server: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult> {
        let registry = self.mcp_registry().await;
        let result = self.mcp.call(&registry, server, tool, arguments).await;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                if lost_login(&error) {
                    self.drop_lost_mcp_login(server).await;
                }
                return Err(error);
            }
        };
        if result.is_error {
            // Error text says what went wrong; it is clipped in case it echoes content.
            let reason: String = result
                .content
                .iter()
                .filter_map(|content| content.as_text())
                .flat_map(str::chars)
                .take(200)
                .collect();
            bail!("{server} {tool} returned an error: {reason}");
        }
        Ok(result)
    }

    /// Disconnects and forgets the server and its tokens; whether it was connected.
    pub async fn remove_mcp(&self, name: &str) -> Result<bool> {
        let existed = self
            .connected_servers()
            .await?
            .iter()
            .any(|server| server == name);
        let registry = self.mcp_registry().await;
        let mut forgotten = false;
        if registry.get_config(name).await.is_some() {
            // Closing can wait on a stuck call; the row goes either way.
            match self
                .mcp
                .bounded(&registry, name, registry.remove(name))
                .await
            {
                Ok(()) => forgotten = true,
                Err(error) => {
                    log::warn!("mcp: unable to close {name}, forgetting it anyway: {error:#}");
                }
            }
        }
        if !forgotten {
            self.delete_mcp_server(name).await?;
        }
        registry.drain_pending_ops();
        Ok(existed)
    }

    /// Reconnects every connected server once, which refreshes and saves expired tokens.
    pub(crate) async fn keep_mcp_alive(&self) {
        let connected = match self.connected_servers().await {
            Ok(connected) if !connected.is_empty() => connected,
            Ok(_) => return,
            Err(error) => {
                log::warn!("mcp: keep-alive unable to list servers: {error:#}");
                return;
            }
        };
        for name in connected {
            // Asked again each time: one that runs out of time is replaced.
            let registry = self.mcp_registry().await;
            let reconnected = self
                .mcp
                .bounded(&registry, &name, registry.reconnect(&name))
                .await;
            registry.drain_pending_ops();
            if let Err(error) = reconnected {
                log::warn!("mcp: keep-alive reconnect failed; server={name} error={error:#}");
                if lost_login(&error) {
                    self.drop_lost_mcp_login(&name).await;
                }
            }
        }
    }
}

/// The Fastmail accounts among `rows` that finished signing in, oldest first.
fn fastmail_rows(rows: &[McpServerRow]) -> Vec<FastmailAccount> {
    rows.iter()
        .filter(|row| row.credentials.is_some() && is_fastmail(&row.name))
        .map(|row| FastmailAccount {
            name: row.name.clone(),
            address: row.address.clone().filter(|address| !address.is_empty()),
        })
        .collect()
}

/// The `state` an authorization URL carries.
fn state_of(auth_url: &str) -> Option<String> {
    Url::parse(auth_url)
        .ok()?
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
}

/// What the app lists for an added server's row.
fn server_info(row: &McpServerRow) -> Option<McpServer> {
    let custom = row.custom.as_ref()?;
    let defs = config_of(row, custom).cached_tools;
    let tools = enabled_names(&row.name, &defs, custom.tools.as_deref());
    Some(info(&row.name, custom, ready(row), tools))
}

/// Why a custom server that offers no usable sign-in was not added.
fn no_sign_in(label: &str) -> String {
    format!(
        "Beep could not connect to {label}. It needs a sign-in Beep does not support, \
         or the URL is wrong."
    )
}

#[cfg(test)]
mod tests;
