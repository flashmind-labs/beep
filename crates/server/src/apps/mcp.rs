//! Remote MCP servers each user connects with OAuth, starting with Fastmail.
//!
//! The phone only catches the OAuth redirect on a localhost port and hands the
//! code to the server. Tokens live in `mcp_servers` and never reach the phone.
//! Each user gets one long-lived `McpRegistry`, and a keep-alive task
//! reconnects every few hours so access tokens refresh and rotated refresh
//! tokens are saved before they lapse.

use crate::user::User;
use anyhow::{Result, bail};
use flashmind::tools::mcp::{McpAuthRequired, McpConfigProvider, McpServerConfig};
use reqwest::Url;
use serde_json::Value;
use std::{
    sync::{Arc, Weak},
    time::Duration,
};

pub const FASTMAIL: &str = "fastmail";

pub(crate) const KEEP_ALIVE: Duration = Duration::from_secs(6 * 60 * 60);

/// The fixed part of a known server's config. Stored rows only add cached
/// tools and credentials, so a row can never point the server at another URL.
pub(crate) fn preset(name: &str) -> Option<McpServerConfig> {
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

/// Reads and writes one user's rows in `mcp_servers`. Weak, since the
/// user owns the registry that owns this.
pub(crate) struct StoreProvider {
    pub(crate) user: Weak<User>,
}

impl StoreProvider {
    fn user(&self) -> Result<Arc<User>> {
        self.user
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("user is gone"))
    }
}

#[async_trait::async_trait]
impl McpConfigProvider for StoreProvider {
    /// Servers the user finished connecting. Half-started flows are left out.
    async fn list_configs(&self) -> Result<Vec<McpServerConfig>> {
        let mut configs = Vec::new();
        for row in self.user()?.mcp_servers().await? {
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
        self.user()?.save_mcp_config(&config.name, value).await
    }

    async fn delete_config(&self, name: &str) -> Result<()> {
        self.user()?.delete_mcp_server(name).await?;
        Ok(())
    }

    async fn save_credentials(&self, name: &str, credentials: &Value) -> Result<()> {
        self.user()?
            .save_mcp_credentials(name, credentials.clone())
            .await
    }

    async fn load_credentials(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .user()?
            .mcp_servers()
            .await?
            .into_iter()
            .find(|row| row.name == name)
            .and_then(|row| row.credentials))
    }
}

/// Display name for the notice when a server's login is lost.
pub(crate) fn display_name(name: &str) -> &str {
    match name {
        FASTMAIL => "Fastmail",
        _ => name,
    }
}

/// True when `error` means the server rejected the saved login, so only
/// connecting again from the phone can fix it.
pub(crate) fn lost_login(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<McpAuthRequired>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;

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
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = User::for_test(store, "alice");
        let provider = StoreProvider {
            user: Arc::downgrade(&user),
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
