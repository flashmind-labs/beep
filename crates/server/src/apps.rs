//! The curated app list and its Composio connections.
//!
//! Each user's Composio entity id is their user id, the same one the agent
//! uses when it calls Composio tools. Errors reaching the client never carry
//! provider bodies or keys: they become `ApiError::Internal`, logged here.

use crate::{
    AsyncMutex,
    protocol::{ApiError, AppIntegration, AppKind, ConnectLink},
};
use anyhow::{Context, Result, bail};
use flashmind::tools::composio::{ComposioClient, SessionToolkits};
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
    fastmail: bool,
    http: reqwest::Client,
    logos: AsyncMutex<Option<LogoCache>>,
}

impl Apps {
    pub fn new(composio: Option<ComposioSettings>, fastmail: bool) -> Self {
        Self {
            composio,
            fastmail,
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

    pub async fn list(&self, user: &str) -> Result<Vec<AppIntegration>, ApiError> {
        let (connected, logos) = match self.client(user) {
            Some(client) => {
                let (connected, logos) = tokio::join!(connected_slugs(&client, user), self.logos());
                (
                    connected.map_err(internal("list connected accounts"))?,
                    logos,
                )
            }
            None => (HashSet::new(), HashMap::new()),
        };
        let composio = self.composio.is_some();
        Ok(CATALOG
            .iter()
            .map(|app| {
                let (configured, connected) = match app.kind {
                    AppKind::Composio {} => (composio, connected.contains(app.slug)),
                    AppKind::Fastmail {} => (self.fastmail, self.fastmail),
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

    pub async fn disconnect(&self, user: &str, slug: &str) -> Result<(), ApiError> {
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
                "Fastmail is set up on the server with an API token, not from the app.",
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
        log::warn!("composio: unable to {what}: {error:#}");
        ApiError::Internal {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lists_catalog_without_composio_key() {
        let apps = Apps::new(None, true).list("alice").await.unwrap();
        let slugs: Vec<&str> = apps.iter().map(|a| a.slug.as_str()).collect();
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
        let (composio, fastmail) = apps.split_at(9);
        assert!(
            composio
                .iter()
                .all(|a| !a.connected && !a.configured && a.logo_url.is_none())
        );
        assert!(fastmail[0].connected && fastmail[0].configured);
        assert_eq!(fastmail[0].kind, AppKind::Fastmail {});
    }

    #[tokio::test]
    async fn connect_explains_missing_setup() {
        let apps = Apps::new(None, false);
        let error = apps.connect("alice", "gmail").await.unwrap_err();
        assert!(
            matches!(error, ApiError::BadRequest { message } if message.contains("not configured"))
        );
        let error = apps.connect("alice", "fastmail").await.unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { message } if message.contains("server")));
        let error = apps.disconnect("alice", "myspace").await.unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));
    }
}
