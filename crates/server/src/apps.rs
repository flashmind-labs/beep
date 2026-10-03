//! The curated app list, its Composio connections and the MCP servers users
//! connect with OAuth.
//!
//! Each user's Composio entity id is their user id, the same one the agent
//! uses when it calls Composio tools. Errors reaching the client never carry
//! provider bodies or keys: they become `ApiError::Internal`, logged here.

use crate::{
    AsyncMutex,
    mcp::{self, McpConnections},
    protocol::{ApiError, AppIntegration, AppKind, ConnectLink},
};
use anyhow::{Context, Result, bail};
use flashmind::{
    tools::composio::{ComposioClient, SessionToolkits, make_composio_tool_wrappers},
    types::Tool,
};
use futures::future::join_all;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

const COMPOSIO_URL: &str = "https://backend.composio.dev/api/v3.1";
const LOGO_TTL: Duration = Duration::from_secs(60 * 60);
const METADATA_TIMEOUT: Duration = Duration::from_secs(5);
const TOOLS_TIMEOUT: Duration = Duration::from_secs(15);

/// Connection states Composio reports for accounts that are not usable.
const DEAD_STATES: &[&str] = &["EXPIRED", "REVOKED", "FAILED", "DELETED", "INACTIVE"];

/// One entry of the curated list.
struct CatalogApp {
    slug: &'static str,
    name: &'static str,
    description: &'static str,
    kind: AppKind,
}

const fn composio(slug: &'static str, name: &'static str, description: &'static str) -> CatalogApp {
    CatalogApp {
        slug,
        name,
        description,
        kind: AppKind::Composio {},
    }
}

/// Shown in this order.
const CATALOG: &[CatalogApp] = &[
    composio("gmail", "Gmail", "Read, search and send email"),
    composio(
        "googlecalendar",
        "Google Calendar",
        "See and schedule events",
    ),
    composio("notion", "Notion", "Search and update pages"),
    composio("googledrive", "Google Drive", "Find and read your files"),
    composio("slack", "Slack", "Read and send messages"),
    composio("github", "GitHub", "Issues, pull requests and repos"),
    composio("linear", "Linear", "Track and update issues"),
    composio("todoist", "Todoist", "Manage tasks and projects"),
    composio("spotify", "Spotify", "Control music and playlists"),
    CatalogApp {
        slug: "fastmail",
        name: "Fastmail",
        description: "Email, contacts and calendar",
        kind: AppKind::Fastmail {},
    },
];

fn catalog_app(slug: &str) -> Option<&'static CatalogApp> {
    CATALOG.iter().find(|app| app.slug == slug)
}

/// Composio credentials from the server config.
pub struct ComposioSettings {
    pub key: SecretString,
    /// Where Composio sends the browser after OAuth. Composio appends
    /// `status` and `connected_account_id` query parameters. Without it,
    /// Composio shows its own page.
    pub callback_url: Option<String>,
}

struct LogoCache {
    fetched: Instant,
    logos: HashMap<String, String>,
}

/// Toolkit metadata from `GET /toolkits/{slug}`; only the logo is used.
#[derive(Deserialize)]
struct ToolkitDetails {
    #[serde(default)]
    meta: Option<ToolkitMeta>,
}

#[derive(Deserialize)]
struct ToolkitMeta {
    #[serde(default)]
    logo: Option<String>,
}

pub struct Apps {
    composio: Option<ComposioSettings>,
    mcp: Arc<McpConnections>,
    http: reqwest::Client,
    logos: AsyncMutex<Option<LogoCache>>,
}

impl Apps {
    pub fn new(composio: Option<ComposioSettings>, mcp: Arc<McpConnections>) -> Self {
        Self {
            composio,
            mcp,
            http: reqwest::Client::new(),
            logos: AsyncMutex::new(None),
        }
    }

    /// A Composio client acting for `user`, if the server has a key.
    pub fn client(&self, user: &str) -> Option<Arc<ComposioClient>> {
        let settings = self.composio.as_ref()?;
        let key = settings.key.expose_secret().to_owned();
        Some(Arc::new(
            ComposioClient::new(key, None, None).with_entity_id(user.to_owned()),
        ))
    }

    /// Agent tools for the Composio apps `user` has connected. Only the
    /// connected toolkits are listed: the unfiltered catalog is thousands of
    /// tools across many pages. A slow or failing Composio gives no tools
    /// rather than holding up the run.
    pub async fn tools(&self, user: &str) -> Vec<Arc<dyn Tool>> {
        let Some(client) = self.client(user) else {
            return Vec::new();
        };
        let fetch = async {
            let toolkits: Vec<String> = connected_slugs(&client, user)
                .await?
                .into_iter()
                .filter(|slug| {
                    catalog_app(slug).is_some_and(|app| app.kind == AppKind::Composio {})
                })
                .collect();
            if toolkits.is_empty() {
                return Ok(Vec::new());
            }
            client.list_tools_tagged(&toolkits, &["important"]).await
        };
        match tokio::time::timeout(TOOLS_TIMEOUT, fetch).await {
            Ok(Ok(defs)) => make_composio_tool_wrappers(&client, &defs),
            Ok(Err(error)) => {
                log::warn!("apps: unable to list Composio tools: {error:#}");
                Vec::new()
            }
            Err(_) => {
                log::warn!("apps: listing Composio tools timed out");
                Vec::new()
            }
        }
    }

    pub async fn list(&self, user: &str) -> Result<Vec<AppIntegration>, ApiError> {
        // A Composio outage or bad key marks its apps unavailable instead of
        // failing the list, so apps that do not use Composio still show.
        let (connected, logos) = match self.client(user) {
            Some(client) => {
                let (connected, logos) = tokio::join!(connected_slugs(&client, user), self.logos());
                let connected = connected
                    .map_err(|error| {
                        log::warn!("apps: unable to list connected accounts: {error:#}")
                    })
                    .ok();
                (connected, logos)
            }
            None => (None, HashMap::new()),
        };
        let composio = connected.is_some();
        let connected = connected.unwrap_or_default();
        let connected_mcp = self
            .mcp
            .connected(user)
            .await
            .map_err(internal("list MCP servers"))?;
        Ok(CATALOG
            .iter()
            .map(|app| {
                let (configured, connected) = match app.kind {
                    AppKind::Composio {} => (composio, connected.contains(app.slug)),
                    AppKind::Fastmail {} => (true, connected_mcp.contains(app.slug)),
                };
                AppIntegration {
                    slug: app.slug.to_owned(),
                    name: app.name.to_owned(),
                    description: app.description.to_owned(),
                    logo_url: logos.get(app.slug).cloned(),
                    connected,
                    configured,
                    kind: app.kind,
                }
            })
            .collect())
    }

    pub async fn connect(&self, user: &str, slug: &str) -> Result<ConnectLink, ApiError> {
        let client = self.composio_app(user, slug)?;
        let callback = self
            .composio
            .as_ref()
            .and_then(|settings| settings.callback_url.as_deref());
        let url = connect_url(&client, user, slug, callback)
            .await
            .map_err(internal("start OAuth"))?;
        Ok(ConnectLink { url })
    }

    /// Starts OAuth for an MCP app. The browser comes back to `redirect_uri`
    /// on the phone, which passes the result to `finish_auth`.
    pub async fn start_auth(
        &self,
        user: &str,
        slug: &str,
        redirect_uri: &str,
    ) -> Result<ConnectLink, ApiError> {
        mcp_app(slug)?;
        mcp::check_redirect(redirect_uri)
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
        let url = self
            .mcp
            .start_auth(user, slug, redirect_uri)
            .await
            .map_err(internal("start MCP OAuth"))?;
        Ok(ConnectLink { url })
    }

    pub async fn finish_auth(
        &self,
        user: &str,
        slug: &str,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<(), ApiError> {
        mcp_app(slug)?;
        self.mcp
            .finish_auth(user, slug, code, state, issuer)
            .await
            .map_err(internal("finish MCP OAuth"))
    }

    pub async fn disconnect(&self, user: &str, slug: &str) -> Result<(), ApiError> {
        if catalog_app(slug).is_some_and(|app| app.kind == (AppKind::Fastmail {})) {
            let removed = self
                .mcp
                .remove(user, slug)
                .await
                .map_err(internal("remove MCP server"))?;
            return if removed {
                Ok(())
            } else {
                Err(ApiError::NotFound {})
            };
        }
        let client = self.composio_app(user, slug)?;
        let accounts = client
            .list_connected_accounts_for_user(Some(slug), Some(user))
            .await
            .map_err(internal("list connected accounts"))?;
        let ids: Vec<String> = accounts
            .into_iter()
            .filter(|account| account.user_id.as_deref().is_none_or(|id| id == user))
            .filter(|account| account.toolkit_slug().is_none_or(|s| s == slug))
            .map(|account| account.id)
            .collect();
        if ids.is_empty() {
            return Err(ApiError::NotFound {});
        }
        for id in ids {
            client
                .delete_connected_account(&id)
                .await
                .map_err(internal("delete connected account"))?;
        }
        Ok(())
    }

    /// The client for a Composio app the user may connect or disconnect.
    fn composio_app(&self, user: &str, slug: &str) -> Result<Arc<ComposioClient>, ApiError> {
        let app = catalog_app(slug).ok_or_else(|| ApiError::bad_request("unknown app"))?;
        if app.kind == (AppKind::Fastmail {}) {
            return Err(ApiError::bad_request(
                "Fastmail connects through StartAppAuth with a localhost redirect.",
            ));
        }
        self.client(user).ok_or_else(|| {
            ApiError::bad_request("App connections are not configured on this server.")
        })
    }

    /// Logo URLs by slug, refreshed at most once an hour. Failures leave
    /// logos out rather than failing the list.
    async fn logos(&self) -> HashMap<String, String> {
        let Some(settings) = &self.composio else {
            return HashMap::new();
        };
        let mut cache = self.logos.lock().await;
        if let Some(cached) = cache.as_ref().filter(|c| c.fetched.elapsed() < LOGO_TTL) {
            return cached.logos.clone();
        }
        let slugs = CATALOG
            .iter()
            .filter(|app| app.kind == (AppKind::Composio {}))
            .map(|app| app.slug);
        let fetched = join_all(slugs.map(|slug| async move {
            let logo = self.fetch_logo(&settings.key, slug).await;
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

    async fn fetch_logo(&self, key: &SecretString, slug: &str) -> Result<Option<String>> {
        let response = self
            .http
            .get(format!("{COMPOSIO_URL}/toolkits/{slug}"))
            .header("x-api-key", key.expose_secret())
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
fn mcp_app(slug: &str) -> Result<(), ApiError> {
    match catalog_app(slug) {
        Some(app) if app.kind == (AppKind::Fastmail {}) => Ok(()),
        Some(_) => Err(ApiError::bad_request(
            "This app connects through ConnectApp.",
        )),
        None => Err(ApiError::bad_request("unknown app")),
    }
}

/// Slugs of the toolkits `user` has a usable account for.
async fn connected_slugs(client: &ComposioClient, user: &str) -> Result<HashSet<String>> {
    let accounts = client
        .list_connected_accounts_for_user(None, Some(user))
        .await?;
    Ok(accounts
        .iter()
        .filter(|account| account.user_id.as_deref().is_none_or(|id| id == user))
        .filter(|account| {
            account
                .status
                .as_deref()
                .is_none_or(|status| !DEAD_STATES.contains(&status.to_ascii_uppercase().as_str()))
        })
        .filter_map(|account| account.toolkit_slug().map(str::to_owned))
        .collect())
}

/// An OAuth URL for `slug`. A Tool Router session scoped to the toolkit uses
/// the project's auth config when there is one and Composio-managed auth
/// otherwise.
async fn connect_url(
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
fn internal(what: &'static str) -> impl Fn(anyhow::Error) -> ApiError {
    move |error| {
        log::warn!("apps: unable to {what}: {error:#}");
        ApiError::Internal {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use serde_json::json;

    async fn apps() -> (Apps, Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("db.sqlite")).await.unwrap();
        let mcp = Arc::new(McpConnections::new(store.clone()));
        (Apps::new(None, mcp), store, dir)
    }

    #[tokio::test]
    async fn lists_catalog_without_composio_key() {
        let (apps, store, _dir) = apps().await;
        let list = apps.list("alice").await.unwrap();
        let slugs: Vec<&str> = list.iter().map(|a| a.slug.as_str()).collect();
        assert_eq!(
            slugs,
            [
                "gmail",
                "googlecalendar",
                "notion",
                "googledrive",
                "slack",
                "github",
                "linear",
                "todoist",
                "spotify",
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

        store
            .save_mcp_credentials("alice", "fastmail", json!({"client_id": "c"}))
            .await
            .unwrap();
        let list = apps.list("alice").await.unwrap();
        assert!(list[9].connected);
        assert!(!apps.list("bob").await.unwrap()[9].connected);
    }

    #[tokio::test]
    async fn connect_explains_missing_setup() {
        let (apps, _store, _dir) = apps().await;
        let error = apps.connect("alice", "gmail").await.unwrap_err();
        assert!(
            matches!(error, ApiError::BadRequest { message } if message.contains("not configured"))
        );
        let error = apps.connect("alice", "fastmail").await.unwrap_err();
        assert!(
            matches!(error, ApiError::BadRequest { message } if message.contains("StartAppAuth"))
        );
        let error = apps.disconnect("alice", "myspace").await.unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));
    }

    #[tokio::test]
    async fn mcp_auth_checks_app_and_redirect() {
        let (apps, store, _dir) = apps().await;
        let error = apps
            .start_auth("alice", "gmail", "http://localhost:4000/callback")
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));
        let error = apps
            .start_auth("alice", "fastmail", "https://example.com/callback")
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));

        let error = apps.disconnect("alice", "fastmail").await.unwrap_err();
        assert!(matches!(error, ApiError::NotFound {}));
        store
            .save_mcp_credentials("alice", "fastmail", json!({"client_id": "c"}))
            .await
            .unwrap();
        apps.disconnect("alice", "fastmail").await.unwrap();
        assert!(store.mcp_servers("alice").await.unwrap().is_empty());
    }
}
