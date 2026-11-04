//! The Composio client: toolkits, connected accounts and logos.

use super::{CATALOG, catalog_app};
use crate::{
    AsyncMutex,
    protocol::{ApiError, AppKind},
};
use anyhow::{Context, Result, bail};
use flashmind::tools::composio::{
    ComposioClient, ComposioToolDef, ConnectedAccountInfo, SessionToolkits,
};
use futures::future::join_all;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

pub(super) const COMPOSIO_URL: &str = "https://backend.composio.dev/api/v3.1";

pub(super) const LOGO_TTL: Duration = Duration::from_secs(60 * 60);

pub(super) const METADATA_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) const TOOLS_TIMEOUT: Duration = Duration::from_secs(15);

/// Connection states Composio reports for accounts that are not usable.
pub(super) const DEAD_STATES: &[&str] = &["EXPIRED", "REVOKED", "FAILED", "DELETED", "INACTIVE"];

/// Composio credentials from the server config.
pub struct ComposioSettings {
    pub key: SecretString,
    /// Where Composio sends the browser after OAuth, with `status` and `connected_account_id` appended.
    pub callback_url: Option<String>,
}

pub(crate) struct LogoCache {
    pub(super) fetched: Instant,
    pub(super) logos: HashMap<String, String>,
}

/// Toolkit metadata from `GET /toolkits/{slug}`; only the logo is used.
#[derive(Deserialize)]
pub(super) struct ToolkitDetails {
    #[serde(default)]
    pub(super) meta: Option<ToolkitMeta>,
}

#[derive(Deserialize)]
pub(super) struct ToolkitMeta {
    #[serde(default)]
    pub(super) logo: Option<String>,
}

/// A page of `GET /tools`.
#[derive(Deserialize)]
pub(super) struct ToolList {
    #[serde(default)]
    pub(super) items: Vec<ToolItem>,
}

#[derive(Deserialize)]
pub(super) struct ToolItem {
    pub(super) slug: String,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) input_parameters: Value,
    #[serde(default)]
    pub(super) toolkit: Option<ToolkitRef>,
    #[serde(default)]
    pub(super) tags: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct ToolkitRef {
    pub(super) slug: String,
}

impl From<ToolItem> for ComposioToolDef {
    fn from(item: ToolItem) -> Self {
        Self {
            slug: item.slug,
            display_name: item.name,
            description: item.description,
            input_schema: item.input_parameters,
            toolkit_slug: item.toolkit.map(|toolkit| toolkit.slug),
            tags: item.tags,
        }
    }
}

/// The Composio account every user shares, with its logo cache.
pub struct Composio {
    pub(crate) settings: ComposioSettings,
    pub(crate) http: reqwest::Client,
    pub(crate) logos: AsyncMutex<Option<LogoCache>>,
}

impl Composio {
    pub fn new(settings: ComposioSettings) -> Self {
        Self {
            settings,
            http: reqwest::Client::new(),
            logos: AsyncMutex::new(None),
        }
    }

    /// Logo URLs by slug, refreshed at most hourly; failures leave logos out.
    pub(crate) async fn logos(&self) -> HashMap<String, String> {
        let mut cache = self.logos.lock().await;
        if let Some(cached) = cache.as_ref().filter(|c| c.fetched.elapsed() < LOGO_TTL) {
            return cached.logos.clone();
        }
        let slugs = CATALOG
            .iter()
            .filter(|app| app.kind == (AppKind::Composio {}))
            .map(|app| app.slug);
        let fetched = join_all(slugs.map(|slug| async move {
            let logo = self.fetch_logo(slug).await;
            (slug, logo)
        }))
        .await;
        let mut logos = HashMap::new();
        let mut failures = 0;
        for (slug, logo) in fetched {
            match logo {
                Ok(Some(url)) => {
                    logos.insert(slug.to_owned(), url);
                }
                Ok(None) => {}
                Err(error) => {
                    failures += 1;
                    log::debug!("toolkit metadata failed; slug={slug} error={error:#}");
                }
            }
        }
        if failures > 0 {
            log::warn!("toolkit metadata unavailable for {failures} apps");
        }
        // Keep a partial result only until the next request, so a brief outage
        // does not hide logos for an hour.
        if failures == 0 {
            *cache = Some(LogoCache {
                fetched: Instant::now(),
                logos: logos.clone(),
            });
        }
        logos
    }

    /// The definitions of the Composio tools named in `slugs`, in one request.
    pub(crate) async fn tools(&self, slugs: &[&str]) -> Result<Vec<ComposioToolDef>> {
        if slugs.is_empty() {
            return Ok(Vec::new());
        }
        let response = self
            .http
            .get(format!("{COMPOSIO_URL}/tools"))
            .header("x-api-key", self.settings.key.expose_secret())
            .query(&[
                ("tool_slugs", slugs.join(",")),
                ("limit", slugs.len().to_string()),
            ])
            .timeout(TOOLS_TIMEOUT)
            .send()
            .await
            .context("request failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Composio returned {status}");
        }
        let page: ToolList = response.json().await.context("invalid tool list")?;
        Ok(page.items.into_iter().map(ComposioToolDef::from).collect())
    }

    pub(super) async fn fetch_logo(&self, slug: &str) -> Result<Option<String>> {
        let response = self
            .http
            .get(format!("{COMPOSIO_URL}/toolkits/{slug}"))
            .header("x-api-key", self.settings.key.expose_secret())
            .timeout(METADATA_TIMEOUT)
            .send()
            .await
            .context("request failed")?;
        let status = response.status();
        if !status.is_success() {
            // The body is not read: it is the provider's and stays out of logs.
            bail!("Composio returned {status}");
        }
        let details: ToolkitDetails = response.json().await.context("invalid toolkit")?;
        Ok(details
            .meta
            .and_then(|meta| meta.logo)
            .filter(|logo| logo.starts_with("https://")))
    }
}

/// Checks that `slug` is an app connected over MCP OAuth.
pub(crate) fn mcp_app(slug: &str) -> Result<(), ApiError> {
    match catalog_app(slug) {
        Some(app) if app.kind == (AppKind::Fastmail {}) => Ok(()),
        Some(_) => Err(ApiError::bad_request(
            "This app connects through ConnectApp.",
        )),
        None => Err(ApiError::bad_request("unknown app")),
    }
}

/// Slugs of the toolkits `user` has a usable account for.
pub(crate) async fn connected_slugs(
    client: &ComposioClient,
    user: &str,
) -> Result<HashSet<String>> {
    Ok(connected_accounts(client, user)
        .await?
        .iter()
        .filter_map(|account| account.toolkit_slug().map(str::to_owned))
        .collect())
}

/// The usable Composio accounts of `user`.
pub(crate) async fn connected_accounts(
    client: &ComposioClient,
    user: &str,
) -> Result<Vec<ConnectedAccountInfo>> {
    let accounts = client
        .list_connected_accounts_for_user(None, Some(user))
        .await?;
    Ok(accounts
        .into_iter()
        .filter(|account| account.user_id.as_deref().is_none_or(|id| id == user))
        .filter(|account| {
            account
                .status
                .as_deref()
                .is_none_or(|status| !DEAD_STATES.contains(&status.to_ascii_uppercase().as_str()))
        })
        .collect())
}

/// An OAuth URL for `slug`, via a Tool Router session scoped to the toolkit.
pub(crate) async fn connect_url(
    client: &ComposioClient,
    user: &str,
    slug: &str,
    callback: Option<&str>,
) -> Result<String> {
    let auth_config = client
        .list_auth_configs(Some(slug))
        .await?
        .into_iter()
        .next()
        .map(|config| HashMap::from([(slug.to_owned(), config.id)]));
    let toolkits = SessionToolkits {
        enable: Some(vec![slug.to_owned()]),
        disable: None,
    };
    let session = client
        .create_session(user, Some(toolkits), callback, auth_config)
        .await?;
    match client
        .session_link(&session.session_id, slug, callback)
        .await
    {
        Ok(link) => Ok(link.redirect_url),
        Err(error) => match session.connection_urls.get(slug) {
            Some(url) => Ok(url.clone()),
            None => Err(error),
        },
    }
}

/// Logs a provider failure and hides it from the client.
pub(crate) fn internal(what: &'static str) -> impl Fn(anyhow::Error) -> ApiError {
    move |error| {
        log::warn!("apps: unable to {what}: {error:#}");
        ApiError::Internal {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::user::User,
        store::{Store, TestDb},
    };
    use serde_json::json;
    use std::sync::Arc;

    pub(super) async fn alice() -> (Arc<User>, Store, TestDb) {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        (User::for_test(store.clone(), "alice"), store, db)
    }

    #[tokio::test]
    pub(super) async fn lists_catalog_without_composio_key() {
        let (alice, store, _db) = alice().await;
        let list = alice.apps().await.unwrap();
        let slugs: Vec<&str> = list.iter().map(|a| a.slug.as_str()).collect();
        assert_eq!(
            slugs,
            [
                "gmail",
                "googlecalendar",
                "notion",
                "googledrive",
                "outlook",
                "microsoft_teams",
                "one_drive",
                "excel",
                "slack",
                "fastmail"
            ]
        );
        let (composio, fastmail) = list.split_at(9);
        assert!(
            composio
                .iter()
                .all(|a| !a.connected && !a.configured && a.logo_url.is_none())
        );
        assert!(!fastmail[0].connected && fastmail[0].configured);
        assert_eq!(fastmail[0].kind, AppKind::Fastmail {});

        alice
            .save_mcp_credentials("fastmail", json!({"client_id": "c"}))
            .await
            .unwrap();
        let list = alice.apps().await.unwrap();
        assert!(list[9].connected);
        User::for_test(store.clone(), "bob")
            .save_mcp_credentials("fastmail", json!({"client_id": "c"}))
            .await
            .unwrap();
        assert!(alice.apps().await.unwrap()[9].connected);
    }

    #[tokio::test]
    pub(super) async fn mcp_auth_checks_app_and_redirect() {
        let (alice, _store, _db) = alice().await;
        let error = alice
            .start_app_auth("gmail", "http://localhost:4000/callback", false)
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));
        let error = alice
            .start_app_auth("fastmail", "https://example.com/callback", false)
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));

        let error = alice.disconnect_app("fastmail").await.unwrap_err();
        assert!(matches!(error, ApiError::NotFound {}));
        alice
            .save_mcp_credentials("fastmail", json!({"client_id": "c"}))
            .await
            .unwrap();
        alice.disconnect_app("fastmail").await.unwrap();
        assert!(alice.mcp_servers().await.unwrap().is_empty());
    }
}
