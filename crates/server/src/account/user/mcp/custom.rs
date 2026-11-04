//! A user's custom MCP servers: adding, signing in, their tools.

use super::{super::User, FLOW_TTL, Flow, no_sign_in, server_info, state_of};
use crate::{
    app::{Reach, UserNotice},
    apps::{
        custom_mcp::{
            self, LIMITS, MAX_LABEL, MAX_SERVERS, MAX_URL, Outcome, check_url, choose, config_of,
            enabled_names, host_of, info, is_custom, recheck, register, server_name, tool_list,
        },
        internal,
        mcp::lost_login,
    },
    protocol::{ApiError, McpConnect, McpServer, McpTools, Notice, mcp_callback_url},
    store::{CustomMcp, McpServerRow},
};
use anyhow::Result;
use serde_json::Value;
use std::time::Instant;

impl User {
    /// The servers the user added by URL.
    pub async fn list_custom_mcp(&self) -> Result<Vec<McpServer>, ApiError> {
        let rows = self
            .mcp_servers()
            .await
            .map_err(internal("list MCP servers"))?;
        Ok(rows.iter().filter_map(server_info).collect())
    }

    /// Adds a server by URL and connects it, or starts its sign-in.
    pub async fn add_custom_mcp(&self, label: &str, url: &str) -> Result<McpConnect, ApiError> {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > MAX_LABEL || url.len() > MAX_URL {
            return Err(ApiError::bad_request(
                "Give the server a short name and its URL.",
            ));
        }
        let name = server_name(label)
            .ok_or_else(|| ApiError::bad_request("The name needs at least one letter or digit."))?;
        let url = check_url(url)
            .await
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        let rows = self
            .mcp_servers()
            .await
            .map_err(internal("list MCP servers"))?;
        if rows.iter().any(|row| row.name == name) {
            return Err(ApiError::bad_request(format!(
                "You already have a server called {label}."
            )));
        }
        if rows.iter().filter(|row| row.custom.is_some()).count() >= MAX_SERVERS {
            return Err(ApiError::bad_request(format!(
                "You can add up to {MAX_SERVERS} servers."
            )));
        }
        let custom = CustomMcp {
            url: url.to_string(),
            label: label.to_owned(),
            sign_in: false,
            tools: None,
        };
        let added = self
            .store
            .add_custom_mcp(&self.key, &name, &custom)
            .await
            .map_err(internal("add MCP server"))?;
        if !added {
            return Err(ApiError::bad_request("That name is taken."));
        }
        let connect = self.connect_custom(&name, custom).await;
        if connect.is_err() {
            // Nothing was saved worth keeping: the user fixes the URL and adds again.
            let _ = self.delete_mcp_server(&name).await;
        }
        connect
    }

    /// Connects an added server again, starting a new sign-in if it needs one.
    pub async fn reconnect_custom_mcp(&self, id: &str) -> Result<McpConnect, ApiError> {
        let row = self.custom_row(id).await?;
        let custom = row.custom.clone().ok_or(ApiError::NotFound {})?;
        if let Some(registry) = self.custom_mcp.get() {
            // Drops the old connection, so the next one starts from the saved login.
            let _ = registry.remove(id).await;
            registry.drain_pending_ops();
        }
        self.connect_custom(id, custom).await
    }

    /// Finishes the sign-in `state` names, which this user started.
    pub async fn finish_custom_mcp(
        &self,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<McpServer, ApiError> {
        let flow = self
            .mcp_flows
            .lock()
            .map_err(|_| ApiError::Internal {})?
            .remove(state);
        let Some(flow) = flow.filter(|flow| flow.started.elapsed() < FLOW_TTL) else {
            return Err(ApiError::bad_request(
                "This sign-in expired or was already used. Connect the server again.",
            ));
        };
        let row = self.custom_row(&flow.server).await?;
        let custom = row.custom.clone().ok_or(ApiError::NotFound {})?;
        recheck(&custom.url)
            .await
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        let host = host_of(&custom.url);
        let permit = LIMITS.begin(&host, self.id()).map_err(|_| {
            ApiError::bad_request(format!("{} is busy. Try again in a moment.", custom.label))
        })?;
        let registry = self.custom_registry().await;
        let result = registry
            .complete_auth(&flow.server, code, state, issuer)
            .await;
        registry.drain_pending_ops();
        if let Err(error) = result {
            permit.finish(custom_mcp::outcome_of(&error));
            log::warn!(
                "mcp: sign-in failed; server={} error={error:#}",
                flow.server
            );
            return Err(ApiError::bad_request(format!(
                "Signing in to {} failed. Try connecting it again.",
                custom.label
            )));
        }
        permit.finish(Outcome::Ok);
        let row = self.custom_row(&flow.server).await?;
        server_info(&row).ok_or(ApiError::NotFound {})
    }

    /// Removes an added server and its login.
    pub async fn remove_custom_mcp(&self, id: &str) -> Result<(), ApiError> {
        self.custom_row(id).await?;
        if let Some(registry) = self.custom_mcp.get() {
            let _ = registry.remove(id).await;
            registry.drain_pending_ops();
        }
        if let Ok(mut flows) = self.mcp_flows.lock() {
            flows.retain(|_, flow| flow.server != id);
        }
        self.delete_mcp_server(id)
            .await
            .map_err(internal("remove MCP server"))?;
        Ok(())
    }

    /// Every tool the added server `id` listed at its last connect.
    pub async fn list_mcp_tools(&self, id: &str) -> Result<McpTools, ApiError> {
        let row = self.custom_row(id).await?;
        let custom = row.custom.as_ref().ok_or(ApiError::NotFound {})?;
        let defs = config_of(&row, custom).cached_tools;
        Ok(tool_list(id, &defs, custom.tools.as_deref()))
    }

    /// Turns one tool of the added server `id` on or off for the agent.
    pub async fn set_mcp_tool(&self, id: &str, tool: &str, on: bool) -> Result<McpTools, ApiError> {
        let row = self.custom_row(id).await?;
        let custom = row.custom.as_ref().ok_or(ApiError::NotFound {})?;
        let defs = config_of(&row, custom).cached_tools;
        if on && !defs.iter().any(|def| def.name == tool) {
            return Err(ApiError::NotFound {});
        }
        let picks = choose(id, &defs, custom.tools.as_deref(), tool, on);
        self.store
            .set_mcp_tools(&self.key, id, &picks)
            .await
            .map_err(internal("save MCP tools"))?;
        Ok(tool_list(id, &defs, Some(&picks)))
    }

    pub(super) async fn custom_row(&self, id: &str) -> Result<McpServerRow, ApiError> {
        if !is_custom(id) {
            return Err(ApiError::NotFound {});
        }
        self.mcp_servers()
            .await
            .map_err(internal("list MCP servers"))?
            .into_iter()
            .find(|row| row.name == id && row.custom.is_some())
            .ok_or(ApiError::NotFound {})
    }

    /// Connects `name`, or starts its sign-in when the server asks for one.
    pub(super) async fn connect_custom(
        &self,
        name: &str,
        mut custom: CustomMcp,
    ) -> Result<McpConnect, ApiError> {
        let busy =
            || ApiError::bad_request(format!("{} is busy. Try again in a moment.", custom.label));
        let host = host_of(&custom.url);
        let permit = LIMITS.begin(&host, self.id()).map_err(|_| busy())?;
        let registry = self.custom_registry().await;
        let row = self.custom_row(name).await?;
        let config = config_of(&row, &custom);
        register(registry, &config)
            .await
            .map_err(internal("register MCP server"))?;
        if !custom.sign_in || row.credentials.is_some() {
            let connected = registry.connect(config.clone()).await;
            registry.drain_pending_ops();
            match connected {
                Ok(tools) => {
                    permit.finish(Outcome::Ok);
                    let tools = enabled_names(name, &tools, custom.tools.as_deref());
                    return Ok(McpConnect {
                        server: info(name, &custom, true, tools),
                        auth_url: None,
                    });
                }
                Err(error) if lost_login(&error) => {}
                Err(error) => {
                    permit.finish(custom_mcp::outcome_of(&error));
                    log::info!("mcp: connect failed; server={name} error={error:#}");
                    return Err(ApiError::bad_request(format!(
                        "Beep could not connect to {}. Check the URL.",
                        custom.label
                    )));
                }
            }
        }
        // The server wants a sign-in: an old login, if any, no longer works.
        if row.credentials.is_some() {
            let _ = self.save_mcp_credentials(name, Value::Null).await;
        }
        let started = registry.start_auth(name, &mcp_callback_url()).await;
        let auth_url = match started {
            Ok(url) => url,
            Err(error) => {
                permit.finish(custom_mcp::outcome_of(&error));
                log::info!("mcp: no sign-in; server={name} error={error:#}");
                return Err(ApiError::bad_request(no_sign_in(&custom.label)));
            }
        };
        permit.finish(Outcome::Ok);
        let Some(state) = state_of(&auth_url) else {
            log::warn!("mcp: sign-in URL without state; server={name}");
            return Err(ApiError::Internal {});
        };
        custom.sign_in = true;
        self.store
            .set_mcp_sign_in(&self.key, name)
            .await
            .map_err(internal("save MCP sign-in"))?;
        if let Ok(mut flows) = self.mcp_flows.lock() {
            flows.retain(|_, flow| flow.started.elapsed() < FLOW_TTL && flow.server != name);
            flows.insert(
                state,
                Flow {
                    server: name.to_owned(),
                    started: Instant::now(),
                },
            );
        }
        Ok(McpConnect {
            server: info(name, &custom, false, Vec::new()),
            auth_url: Some(auth_url),
        })
    }

    /// Clears a rejected login of an added server, which stays listed, and tells the user.
    pub(super) async fn drop_lost_custom_login(&self, name: &str) {
        let Ok(row) = self.custom_row(name).await else {
            return;
        };
        if row.credentials.is_none() {
            return;
        }
        if let Err(error) = self.save_mcp_credentials(name, Value::Null).await {
            log::warn!("mcp: unable to clear a lost login; server={name} error={error:#}");
            return;
        }
        let label = row.custom.map(|custom| custom.label).unwrap_or_default();
        log::warn!("mcp: {name} rejected the saved login, asking to reconnect; user={self}");
        let notice = UserNotice {
            user: self.id().to_owned(),
            notice: Notice {
                title: format!("Reconnect {label}"),
                body: format!("Beep lost access to {label}. Connect it again under Apps."),
                session_id: String::new(),
                app: None,
                approval: None,
                changed: false,
                group: false,
            },
            desktop: None,
            to: Reach::Everyone,
        };
        // Nobody listening just means the phone is offline.
        let _ = self.notifications.send(notice);
    }
}
