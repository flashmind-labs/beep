//! The curated app list, its Composio connections and the MCP servers users
//! connect with OAuth.
//!
//! Each user's Composio entity id is their user id, the same one the agent
//! uses when it calls Composio tools. Errors reaching the client never carry
//! provider bodies or keys: they become `ApiError::Internal`, logged here.

pub mod mcp;

use crate::{
    AsyncMutex,
    feeds::slack::SLACK,
    protocol::{ApiError, AppKind},
};
use anyhow::{Context, Result, bail};
use flashmind::tools::composio::{ComposioClient, ComposioToolDef, SessionToolkits};
use futures::future::join_all;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

const COMPOSIO_URL: &str = "https://backend.composio.dev/api/v3.1";
const LOGO_TTL: Duration = Duration::from_secs(60 * 60);
const METADATA_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const TOOLS_TIMEOUT: Duration = Duration::from_secs(15);

/// Connection states Composio reports for accounts that are not usable.
const DEAD_STATES: &[&str] = &["EXPIRED", "REVOKED", "FAILED", "DELETED", "INACTIVE"];

/// One entry of the curated list.
pub(crate) struct CatalogApp {
    pub(crate) slug: &'static str,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: AppKind,
    /// The Composio tools the agent gets once the app is connected. Every
    /// tool's schema goes out on each model request, and an app's full set
    /// runs to tens of thousands of tokens, so only the common ones are
    /// picked.
    pub(crate) tools: &'static [&'static str],
}

const fn composio(
    slug: &'static str,
    name: &'static str,
    description: &'static str,
    tools: &'static [&'static str],
) -> CatalogApp {
    CatalogApp {
        slug,
        name,
        description,
        kind: AppKind::Composio {},
        tools,
    }
}

/// Shown in this order.
pub(crate) const CATALOG: &[CatalogApp] = &[
    composio(
        "gmail",
        "Gmail",
        "Read, search and send email",
        &[
            "GMAIL_FETCH_EMAILS",
            "GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID",
            "GMAIL_SEND_EMAIL",
            "GMAIL_REPLY_TO_THREAD",
            "GMAIL_CREATE_EMAIL_DRAFT",
            "GMAIL_MOVE_TO_TRASH",
            "GMAIL_SEARCH_PEOPLE",
        ],
    ),
    composio(
        "googlecalendar",
        "Google Calendar",
        "See and schedule events",
        &[
            "GOOGLECALENDAR_EVENTS_LIST_ALL_CALENDARS",
            "GOOGLECALENDAR_CREATE_EVENT",
            "GOOGLECALENDAR_PATCH_EVENT",
            "GOOGLECALENDAR_DELETE_EVENT",
            "GOOGLECALENDAR_FREE_BUSY_QUERY",
        ],
    ),
    composio(
        "notion",
        "Notion",
        "Search and update pages",
        &[
            "NOTION_SEARCH_NOTION_PAGE",
            "NOTION_GET_PAGE_MARKDOWN",
            "NOTION_CREATE_NOTION_PAGE",
            "NOTION_ADD_MULTIPLE_PAGE_CONTENT",
            "NOTION_UPDATE_PAGE",
            "NOTION_QUERY_DATABASE",
            "NOTION_INSERT_ROW_FROM_NL",
        ],
    ),
    composio(
        "googledrive",
        "Google Drive",
        "Find and read your files",
        &[
            "GOOGLEDRIVE_LIST_FILES",
            "GOOGLEDRIVE_GET_FILE_METADATA",
            "GOOGLEDRIVE_EXPORT_GOOGLE_WORKSPACE_FILE",
            "GOOGLEDRIVE_DOWNLOAD_FILE",
        ],
    ),
    composio(
        "outlook",
        "Outlook",
        "Email, calendar and contacts",
        &[
            "OUTLOOK_QUERY_EMAILS",
            "OUTLOOK_SEND_EMAIL",
            "OUTLOOK_REPLY_EMAIL",
            "OUTLOOK_CREATE_DRAFT",
            "OUTLOOK_MOVE_MESSAGE",
            "OUTLOOK_GET_CALENDAR_VIEW",
            "OUTLOOK_CALENDAR_CREATE_EVENT",
            "OUTLOOK_DECLINE_EVENT",
        ],
    ),
    composio(
        "microsoft_teams",
        "Microsoft Teams",
        "Read and send messages",
        &[
            "MICROSOFT_TEAMS_SEARCH_MESSAGES",
            "MICROSOFT_TEAMS_CHATS_GET_ALL_CHATS",
            "MICROSOFT_TEAMS_GET_CHAT_MESSAGE",
            "MICROSOFT_TEAMS_TEAMS_LIST_CHANNEL_MESSAGES",
            "MICROSOFT_TEAMS_TEAMS_POST_CHAT_MESSAGE",
        ],
    ),
    composio(
        "one_drive",
        "OneDrive",
        "Find and read your files",
        &[
            "ONE_DRIVE_SEARCH_DRIVE_ITEMS",
            "ONE_DRIVE_GET_RECENT_ITEMS",
            "ONE_DRIVE_GET_SHARED_ITEMS",
            "ONE_DRIVE_GET_ITEM",
            "ONE_DRIVE_DOWNLOAD_FILE",
        ],
    ),
    composio(
        "excel",
        "Excel",
        "Read and edit workbooks",
        &[
            "EXCEL_SEARCH_FILES",
            "EXCEL_GET_WORKSHEET_USED_RANGE",
            "EXCEL_LIST_TABLE_COLUMNS",
            "EXCEL_INSERT_RANGE",
        ],
    ),
    composio(
        "share_point",
        "SharePoint",
        "Sites, lists and documents",
        &[
            "SHARE_POINT_SEARCH_FILES",
            "SHARE_POINT_LIST_SITES",
            "SHARE_POINT_GET_SITE_PAGE_CONTENT",
            "SHARE_POINT_GET_FILE_BY_SERVER_RELATIVE_URL",
            "SHARE_POINT_CREATE_LIST_ITEM_BY_ID",
            "SHARE_POINT_UPDATE_LIST_ITEM",
        ],
    ),
    composio(
        SLACK,
        "Slack",
        "Read and send messages",
        &[
            "SLACK_SEARCH_MESSAGES",
            "SLACK_FETCH_CONVERSATION_HISTORY",
            "SLACK_LIST_UNREAD_CHANNEL_MESSAGES",
            "SLACK_FIND_CHANNELS",
            "SLACK_FIND_USERS",
            "SLACK_SEND_MESSAGE",
        ],
    ),
    composio(
        "github",
        "GitHub",
        "Issues, pull requests and repos",
        &[
            "GITHUB_SEARCH_ISSUES",
            "GITHUB_GET_AN_ISSUE",
            "GITHUB_GET_ISSUE_COMMENTS",
            "GITHUB_CREATE_AN_ISSUE",
            "GITHUB_CREATE_AN_ISSUE_COMMENT",
            "GITHUB_LIST_PULL_REQUESTS",
            "GITHUB_GET_A_PULL_REQUEST",
            "GITHUB_LIST_NOTIFICATIONS",
            "GITHUB_GET_REPOSITORY_CONTENT",
        ],
    ),
    composio(
        "linear",
        "Linear",
        "Track and update issues",
        &[
            "LINEAR_SEARCH_ISSUES",
            "LINEAR_GET_LINEAR_ISSUE",
            "LINEAR_CREATE_LINEAR_ISSUE",
            "LINEAR_UPDATE_ISSUE",
            "LINEAR_LIST_COMMENTS",
            "LINEAR_CREATE_LINEAR_COMMENT",
        ],
    ),
    composio(
        "todoist",
        "Todoist",
        "Manage tasks and projects",
        &[
            "TODOIST_FILTER_TASKS",
            "TODOIST_GET_ALL_TASKS",
            "TODOIST_GET_ALL_PROJECTS",
            "TODOIST_CREATE_TASK",
            "TODOIST_UPDATE_TASK",
            "TODOIST_CLOSE_TASK",
        ],
    ),
    CatalogApp {
        slug: "fastmail",
        name: "Fastmail",
        description: "Email, contacts and calendar",
        kind: AppKind::Fastmail {},
        tools: &[],
    },
];

pub(crate) fn catalog_app(slug: &str) -> Option<&'static CatalogApp> {
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

pub(crate) struct LogoCache {
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

/// A page of `GET /tools`.
#[derive(Deserialize)]
struct ToolList {
    #[serde(default)]
    items: Vec<ToolItem>,
}

#[derive(Deserialize)]
struct ToolItem {
    slug: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_parameters: Value,
    #[serde(default)]
    toolkit: Option<ToolkitRef>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct ToolkitRef {
    slug: String,
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

    /// Logo URLs by slug, refreshed at most once an hour. Failures leave
    /// logos out rather than failing the list.
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

    async fn fetch_logo(&self, slug: &str) -> Result<Option<String>> {
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
        store::{Store, TestDb},
        user::User,
    };
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn composio_apps_pick_their_own_tools() {
        for app in CATALOG {
            if app.kind != (AppKind::Composio {}) {
                assert!(app.tools.is_empty(), "{}", app.slug);
                continue;
            }
            assert!(!app.tools.is_empty(), "{} has no tools", app.slug);
            let prefix = format!("{}_", app.slug.to_uppercase());
            for tool in app.tools {
                assert!(
                    tool.starts_with(&prefix),
                    "{tool} is not a {} tool",
                    app.slug
                );
            }
        }
    }

    async fn alice() -> (Arc<User>, Store, TestDb) {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        (User::for_test(store.clone(), "alice"), store, db)
    }

    #[tokio::test]
    async fn lists_catalog_without_composio_key() {
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
                "share_point",
                "slack",
                "github",
                "linear",
                "todoist",
                "fastmail"
            ]
        );
        let (composio, fastmail) = list.split_at(13);
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
        assert!(list[13].connected);
        User::for_test(store.clone(), "bob")
            .save_mcp_credentials("fastmail", json!({"client_id": "c"}))
            .await
            .unwrap();
        assert!(alice.apps().await.unwrap()[13].connected);
    }

    #[tokio::test]
    async fn connect_explains_missing_setup() {
        let (alice, _store, _db) = alice().await;
        let error = alice.connect_app("gmail").await.unwrap_err();
        assert!(
            matches!(error, ApiError::BadRequest { message } if message.contains("not configured"))
        );
        let error = alice.connect_app("fastmail").await.unwrap_err();
        assert!(
            matches!(error, ApiError::BadRequest { message } if message.contains("StartAppAuth"))
        );
        let error = alice.disconnect_app("myspace").await.unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));
    }

    #[tokio::test]
    async fn mcp_auth_checks_app_and_redirect() {
        let (alice, _store, _db) = alice().await;
        let error = alice
            .start_app_auth("gmail", "http://localhost:4000/callback")
            .await
            .unwrap_err();
        assert!(matches!(error, ApiError::BadRequest { .. }));
        let error = alice
            .start_app_auth("fastmail", "https://example.com/callback")
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
