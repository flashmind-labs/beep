//! Remote MCP servers each user connects with OAuth, starting with Fastmail.
//!
//! The phone only catches the OAuth redirect on a localhost port and hands the
//! code to the server. Tokens live in `mcp_servers` and never reach the phone.
//! Each user gets one long-lived `McpRegistry`, and a keep-alive task
//! reconnects every few hours so access tokens refresh and rotated refresh
//! tokens are saved before they lapse.

use crate::{AsyncMutex, app::App, store::Store};
use anyhow::{Result, bail};
use flashmind::{
    tools::mcp::{McpConfigProvider, McpRegistry, McpServerConfig, make_mcp_tool_wrappers},
    types::Tool,
};
use reqwest::Url;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

pub const FASTMAIL: &str = "fastmail";

const KEEP_ALIVE: Duration = Duration::from_secs(6 * 60 * 60);

/// The fixed part of a known server's config. Stored rows only add cached
/// tools and credentials, so a row can never point the server at another URL.
fn preset(name: &str) -> Option<McpServerConfig> {
    match name {
        FASTMAIL => Some(McpServerConfig {
            name: FASTMAIL.to_owned(),
            url: Some("https://api.fastmail.com/mcp".to_owned()),
            ..Default::default()
        }),
        _ => None,
    }
}

/// Accepts only `http://localhost:<port>/...` style loopback redirects, the
/// only kind Fastmail allows and the only kind the phone listens on.
pub fn check_redirect(redirect_uri: &str) -> Result<()> {
    let url = Url::parse(redirect_uri)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "http" || !loopback || url.port().is_none() {
        bail!("redirect must be http on a loopback address with a port");
    }
    Ok(())
}

/// Reads and writes one user's rows in `mcp_servers`.
struct StoreProvider {
    store: Store,
    user: String,
}

#[async_trait::async_trait]
impl McpConfigProvider for StoreProvider {
    /// Servers the user finished connecting. Half-started flows are left out.
    async fn list_configs(&self) -> Result<Vec<McpServerConfig>> {
        let mut configs = Vec::new();
        for row in self.store.mcp_servers(&self.user).await? {
            let (Some(mut config), Some(credentials)) = (preset(&row.name), row.credentials) else {
                continue;
            };
            if let Ok(stored) = serde_json::from_value::<McpServerConfig>(row.config) {
                config.cached_tools = stored.cached_tools;
            }
            config.credentials = Some(credentials);
            configs.push(config);
        }
        Ok(configs)
    }

    /// Flashmind calls this with in-memory copies whose credentials may be
    /// stale, so they are dropped here and only `save_credentials` writes them.
    async fn save_config(&self, config: &McpServerConfig) -> Result<()> {
        if preset(&config.name).is_none() {
            bail!("unknown MCP server");
        }
        let mut config = config.clone();
        config.credentials = None;
        config.bearer_token = None;
        let value = serde_json::to_value(&config)?;
        self.store
            .save_mcp_config(&self.user, &config.name, value)
            .await
    }

    async fn delete_config(&self, name: &str) -> Result<()> {
        self.store.delete_mcp_server(&self.user, name).await?;
        Ok(())
    }

    async fn save_credentials(&self, name: &str, credentials: &Value) -> Result<()> {
        self.store
            .save_mcp_credentials(&self.user, name, credentials.clone())
            .await
    }

    async fn load_credentials(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .store
            .mcp_servers(&self.user)
            .await?
            .into_iter()
            .find(|row| row.name == name)
            .and_then(|row| row.credentials))
    }
}

/// One registry per user, created on first use.
pub struct McpConnections {
    store: Store,
    registries: AsyncMutex<HashMap<String, McpRegistry>>,
}

impl McpConnections {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            registries: AsyncMutex::new(HashMap::new()),
        }
    }

    async fn registry(&self, user: &str) -> McpRegistry {
        let mut registries = self.registries.lock().await;
        if let Some(registry) = registries.get(user) {
            return registry.clone();
        }
        let provider = Arc::new(StoreProvider {
            store: self.store.clone(),
            user: user.to_owned(),
        });
        let registry = McpRegistry::new(provider, None);
        registry.load_saved().await;
        registries.insert(user.to_owned(), registry.clone());
        registry
    }

    /// Names of the servers `user` has credentials for.
    pub async fn connected(&self, user: &str) -> Result<HashSet<String>> {
        Ok(self
            .store
            .mcp_servers(user)
            .await?
            .into_iter()
            .filter(|row| row.credentials.is_some())
            .map(|row| row.name)
            .collect())
    }

    /// Agent tools for every server the user has connected.
    pub async fn tools(&self, user: &str) -> Vec<Arc<dyn Tool>> {
        let connected = match self.connected(user).await {
            Ok(connected) if !connected.is_empty() => connected,
            Ok(_) => return Vec::new(),
            Err(error) => {
                log::warn!("mcp: unable to list servers: {error:#}");
                return Vec::new();
            }
        };
        let registry = self.registry(user).await;
        // Tools are rebuilt from `current_mcp_tools` on every run, so the
        // queued register and unregister ops are not needed.
        registry.drain_pending_ops();
        registry
            .current_mcp_tools()
            .into_iter()
            .filter(|(server, _)| connected.contains(server))
            .flat_map(|(server, defs)| {
                make_mcp_tool_wrappers(
                    &registry,
                    &server,
                    &defs,
                    registry.restrictions_for(&server),
                    registry.session_approved_for(&server),
                )
            })
            .collect()
    }

    /// Starts OAuth and returns the URL to open in the browser.
    pub async fn start_auth(&self, user: &str, name: &str, redirect_uri: &str) -> Result<String> {
        let Some(config) = preset(name) else {
            bail!("unknown MCP server");
        };
        check_redirect(redirect_uri)?;
        let registry = self.registry(user).await;
        registry.add(config).await?;
        registry.start_auth(name, redirect_uri).await
    }

    /// Exchanges the code from the redirect, saves the tokens and connects.
    pub async fn finish_auth(
        &self,
        user: &str,
        name: &str,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<()> {
        let registry = self.registry(user).await;
        registry.complete_auth(name, code, state, issuer).await
    }

    /// Disconnects and forgets the server and its tokens. Returns whether it existed.
    pub async fn remove(&self, user: &str, name: &str) -> Result<bool> {
        let existed = self.connected(user).await?.contains(name);
        let registry = self.registry(user).await;
        if registry.get_config(name).await.is_some() {
            registry.remove(name).await?;
        } else {
            self.store.delete_mcp_server(user, name).await?;
        }
        registry.drain_pending_ops();
        Ok(existed)
    }

    /// Reconnects every connected server once. Connecting refreshes an
    /// expired access token, which also rotates and saves the refresh token.
    async fn keep_alive(&self) {
        let users = match self.store.mcp_users().await {
            Ok(users) => users,
            Err(error) => {
                log::warn!("mcp: keep-alive unable to list users: {error:#}");
                return;
            }
        };
        for user in users {
            let Ok(connected) = self.connected(&user).await else {
                continue;
            };
            let registry = self.registry(&user).await;
            for name in connected {
                if let Err(error) = registry.reconnect(&name).await {
                    log::warn!("mcp: keep-alive reconnect failed; server={name} error={error:#}");
                }
            }
            registry.drain_pending_ops();
        }
    }
}

/// Runs the keep-alive every few hours, starting one period after launch.
pub fn start_keep_alive(app: Arc<App>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(KEEP_ALIVE);
        interval.tick().await;
        loop {
            interval.tick().await;
            app.mcp.keep_alive().await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_must_be_loopback_http_with_port() {
        assert!(check_redirect("http://localhost:43123/callback").is_ok());
        assert!(check_redirect("http://127.0.0.1:43123/callback").is_ok());
        assert!(check_redirect("http://[::1]:43123/callback").is_ok());
        assert!(check_redirect("http://localhost/callback").is_err());
        assert!(check_redirect("https://localhost:443/callback").is_err());
        assert!(check_redirect("http://evil.example:8080/callback").is_err());
        assert!(check_redirect("http://localhost.evil.example:8080/").is_err());
    }

    #[tokio::test]
    async fn provider_keeps_credentials_out_of_config() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.sqlite")).await.unwrap();
        let provider = StoreProvider {
            store: store.clone(),
            user: "alice".to_owned(),
        };

        let mut config = preset(FASTMAIL).unwrap();
        provider.save_config(&config).await.unwrap();
        assert!(provider.list_configs().await.unwrap().is_empty());

        let credentials = serde_json::json!({"client_id": "c", "token_response": null});
        provider
            .save_credentials(FASTMAIL, &credentials)
            .await
            .unwrap();
        // A stale copy without credentials must not clear the saved ones.
        config.url = Some("https://evil.example/mcp".to_owned());
        provider.save_config(&config).await.unwrap();

        let configs = provider.list_configs().await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].credentials, Some(credentials));
        assert_eq!(
            configs[0].url.as_deref(),
            Some("https://api.fastmail.com/mcp")
        );

        provider
            .save_credentials(FASTMAIL, &Value::Null)
            .await
            .unwrap();
        assert!(provider.list_configs().await.unwrap().is_empty());
        assert!(provider.load_credentials(FASTMAIL).await.unwrap().is_none());
    }
}
