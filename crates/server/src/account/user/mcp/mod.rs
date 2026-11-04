//! The user's MCP servers. The registry is built on first use.

use super::User;
use crate::{
    app::{Reach, UserNotice},
    apps::{
        custom_mcp::{
            self, CustomProvider, LIMITS, MAX_LABEL, MAX_SERVERS, MAX_URL, Outcome, check_url,
            choose, config_of, custom_tools, enabled_names, host_of, info, is_custom, ready,
            recheck, register, server_name, tool_list,
        },
        internal,
        mcp::{
            FASTMAIL, FastmailAccount, MAX_FASTMAIL_ACCOUNTS, app_tools, check_redirect,
            display_name, fastmail_name, identity_address, is_fastmail, lost_login, pick_account,
            preset,
        },
    },
    protocol::{ApiError, McpConnect, McpServer, McpTools, Notice, mcp_callback_url},
    store::{CustomMcp, McpServerRow},
};
use anyhow::{Result, bail};
use flashmind::{tools::mcp::McpRegistry, types::Tool};
use reqwest::Url;
use serde_json::{Value, json};
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

    /// The connected Fastmail accounts, oldest (the default) first.
    pub(crate) async fn fastmail_accounts(&self) -> Result<Vec<FastmailAccount>> {
        Ok(fastmail_rows(&self.mcp_servers().await?))
    }

    /// The server of the Fastmail account named by address or id, or of the first.
    pub(crate) async fn fastmail_server(&self, account: Option<&str>) -> Result<String> {
        let accounts = self.fastmail_accounts().await?;
        match pick_account(&accounts, account) {
            Ok(found) => Ok(found.name.clone()),
            Err(message) => bail!(message),
        }
    }

    /// Calls `tool` on the Fastmail account `account` names, or the first.
    pub(crate) async fn call_fastmail(
        &self,
        account: Option<&str>,
        tool: &str,
        arguments: Value,
    ) -> Result<Value> {
        let server = self.fastmail_server(account).await?;
        self.call_mcp(&server, tool, arguments).await
    }

    /// Reads and saves the missing addresses of `accounts` from their sending identities.
    async fn learn_addresses(&self, accounts: &mut [FastmailAccount]) {
        for account in accounts
            .iter_mut()
            .filter(|account| account.address.is_none())
        {
            let Some(address) = self.read_address(&account.name).await else {
                continue;
            };
            if let Err(error) = self
                .store
                .set_mcp_address(&self.key, &account.name, Some(&address))
                .await
            {
                log::warn!("mcp: unable to save the account address: {error:#}");
            }
            account.address = Some(address);
        }
    }

    /// The address Fastmail account `name` sends from, if it answers.
    async fn read_address(&self, name: &str) -> Option<String> {
        match self.call_mcp(name, "list_identities", json!({})).await {
            Ok(value) => identity_address(&value),
            Err(error) => {
                log::info!(
                    "mcp: unable to read the account address; server={name} error={error:#}"
                );
                None
            }
        }
    }

    /// The Fastmail accounts as the app lists them; missing addresses are read in the background.
    pub(crate) async fn list_fastmail_accounts(&self) -> Result<Vec<FastmailAccount>> {
        let accounts = self.fastmail_accounts().await?;
        self.learn_addresses_later(&accounts);
        Ok(accounts)
    }

    /// Reads the addresses `accounts` lack in one background task at a time, each read bounded.
    fn learn_addresses_later(&self, accounts: &[FastmailAccount]) {
        if accounts.iter().all(|account| account.address.is_some()) {
            return;
        }
        let Some(user) = self.me.upgrade() else {
            return;
        };
        if !self.mcp.start_learning() {
            return;
        }
        tokio::spawn(async move {
            match user.fastmail_accounts().await {
                Ok(mut accounts) => user.learn_addresses(&mut accounts).await,
                Err(error) => log::warn!("mcp: unable to list accounts to learn: {error:#}"),
            }
            user.mcp.done_learning();
        });
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

    /// The row of the added server `id`.
    async fn custom_row(&self, id: &str) -> Result<McpServerRow, ApiError> {
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
    async fn connect_custom(
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
    async fn drop_lost_custom_login(&self, name: &str) {
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

    /// Calls one tool outside any run; JSON text in the result is parsed.
    pub async fn call_mcp(&self, server: &str, tool: &str, arguments: Value) -> Result<Value> {
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

    /// Starts a Fastmail sign-in and returns its URL; `add` signs in another account.
    pub async fn start_fastmail_auth(
        &self,
        redirect_uri: &str,
        add: bool,
    ) -> Result<String, ApiError> {
        check_redirect(redirect_uri).map_err(|error| ApiError::bad_request(error.to_string()))?;
        let rows = self
            .mcp_servers()
            .await
            .map_err(internal("list MCP servers"))?;
        let accounts = fastmail_rows(&rows);
        let name = if add || accounts.is_empty() {
            // The first slot without a working login; a half-finished one is reused.
            (1..=MAX_FASTMAIL_ACCOUNTS)
                .map(fastmail_name)
                .find(|name| !accounts.iter().any(|account| &account.name == name))
                .ok_or_else(|| {
                    ApiError::bad_request(format!(
                        "You can connect up to {MAX_FASTMAIL_ACCOUNTS} Fastmail accounts."
                    ))
                })?
        } else {
            accounts[0].name.clone()
        };
        let config = preset(&name).ok_or(ApiError::Internal {})?;
        let registry = self.mcp_registry().await;
        let started = async {
            registry.add(config).await?;
            registry.start_auth(&name, redirect_uri).await
        };
        let auth_url = self
            .mcp
            .bounded(&registry, &name, started)
            .await
            .map_err(internal("start MCP OAuth"))?;
        if let Some(state) = state_of(&auth_url)
            && let Ok(mut flows) = self.mcp_flows.lock()
        {
            flows.retain(|_, flow| flow.started.elapsed() < FLOW_TTL && flow.server != name);
            flows.insert(
                state,
                Flow {
                    server: name,
                    started: Instant::now(),
                },
            );
        }
        Ok(auth_url)
    }

    /// Exchanges the redirect's code and connects its account, replacing an older login to it.
    pub async fn finish_fastmail_auth(
        &self,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<()> {
        let flow = self
            .mcp_flows
            .lock()
            .ok()
            .and_then(|mut flows| flows.remove(state))
            .filter(|flow| is_fastmail(&flow.server));
        let name = flow.map_or_else(|| FASTMAIL.to_owned(), |flow| flow.server);
        let registry = self.mcp_registry().await;
        let completed = registry.complete_auth(&name, code, state, issuer);
        self.mcp.bounded(&registry, &name, completed).await?;
        // The login may be another account's than the slot had before.
        let address = self.read_address(&name).await;
        self.store
            .set_mcp_address(&self.key, &name, address.as_deref())
            .await?;
        let Some(address) = address else {
            return Ok(());
        };
        let older = self
            .fastmail_accounts()
            .await?
            .into_iter()
            .filter(|account| account.name != name && account.address.as_ref() == Some(&address));
        for account in older {
            log::info!("mcp: an account signed in again, dropping its older login; user={self}");
            self.remove_mcp(&account.name).await?;
        }
        Ok(())
    }

    /// Disconnects one Fastmail account, by its id.
    pub async fn remove_fastmail_account(&self, id: &str) -> Result<(), ApiError> {
        if !is_fastmail(id) {
            return Err(ApiError::NotFound {});
        }
        let removed = self
            .remove_mcp(id)
            .await
            .map_err(internal("remove MCP server"))?;
        if removed {
            Ok(())
        } else {
            Err(ApiError::NotFound {})
        }
    }

    /// Disconnects every Fastmail account. Returns whether there was one.
    pub async fn remove_fastmail(&self) -> Result<bool> {
        let names: Vec<String> = self
            .mcp_servers()
            .await?
            .into_iter()
            .map(|row| row.name)
            .filter(|name| is_fastmail(name))
            .collect();
        let mut existed = false;
        for name in names {
            existed |= self.remove_mcp(&name).await?;
        }
        Ok(existed)
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
