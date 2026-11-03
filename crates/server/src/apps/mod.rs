//! The curated app list, its Composio connections and the OAuth MCP servers.
//! Errors reaching the client never carry provider bodies or keys.

pub(crate) mod custom_mcp;
pub(crate) mod files;
pub(crate) mod markdown;
pub mod mcp;

use crate::{
    AsyncMutex,
    feeds::slack::SLACK,
    permissions::Acts,
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
    /// The Composio tools the agent gets; only common ones, since every schema goes out on each request.
    pub(crate) tools: &'static [PickedTool],
}

/// A Composio tool the agent gets, and whether it acts.
pub(crate) struct PickedTool {
    pub(crate) slug: &'static str,
    pub(crate) acts: Acts,
    /// Whether `acts` holds even when Composio's tags say otherwise.
    pub(crate) over_hints: bool,
}

/// A tool that only reads.
pub(crate) const fn read(slug: &'static str) -> PickedTool {
    PickedTool {
        slug,
        acts: Acts::Never,
        over_hints: false,
    }
}

/// A read whose Composio tags wrongly say it creates something.
const fn read_over_hints(slug: &'static str) -> PickedTool {
    PickedTool {
        slug,
        acts: Acts::Never,
        over_hints: true,
    }
}

/// A tool that sends, changes or deletes, so it falls under the app's permission level.
const fn act(slug: &'static str) -> PickedTool {
    PickedTool {
        slug,
        acts: Acts::Always,
        over_hints: false,
    }
}

const fn composio(
    slug: &'static str,
    name: &'static str,
    description: &'static str,
    tools: &'static [PickedTool],
) -> CatalogApp {
    CatalogApp {
        slug,
        name,
        description,
        kind: AppKind::Composio {},
        tools,
    }
}

/// Shown in this order. GitHub, Linear, Todoist and SharePoint are left out (Composio's GitHub was mistaken for gh).
pub(crate) const CATALOG: &[CatalogApp] = &[
    composio(
        "gmail",
        "Gmail",
        "Read, search and send email",
        &[
            read("GMAIL_FETCH_EMAILS"),
            read("GMAIL_FETCH_MESSAGE_BY_MESSAGE_ID"),
            read("GMAIL_GET_ATTACHMENT"),
            act("GMAIL_SEND_EMAIL"),
            act("GMAIL_REPLY_TO_THREAD"),
            act("GMAIL_MOVE_TO_TRASH"),
            act("GMAIL_BATCH_MODIFY_MESSAGES"),
            read("GMAIL_SEARCH_PEOPLE"),
        ],
    ),
    composio(
        "googlecalendar",
        "Google Calendar",
        "See and schedule events",
        &[
            read("GOOGLECALENDAR_EVENTS_LIST_ALL_CALENDARS"),
            act("GOOGLECALENDAR_CREATE_EVENT"),
            act("GOOGLECALENDAR_PATCH_EVENT"),
            act("GOOGLECALENDAR_DELETE_EVENT"),
            read("GOOGLECALENDAR_FREE_BUSY_QUERY"),
        ],
    ),
    composio(
        "notion",
        "Notion",
        "Search and update pages",
        &[
            read("NOTION_SEARCH_NOTION_PAGE"),
            read("NOTION_GET_PAGE_MARKDOWN"),
            act("NOTION_CREATE_NOTION_PAGE"),
            act("NOTION_ADD_MULTIPLE_PAGE_CONTENT"),
            act("NOTION_UPDATE_PAGE"),
            read("NOTION_QUERY_DATABASE"),
            act("NOTION_INSERT_ROW_FROM_NL"),
        ],
    ),
    composio(
        "googledrive",
        "Google Drive",
        "Find and read your files",
        &[
            read("GOOGLEDRIVE_LIST_FILES"),
            read("GOOGLEDRIVE_GET_FILE_METADATA"),
            read("GOOGLEDRIVE_EXPORT_GOOGLE_WORKSPACE_FILE"),
            read("GOOGLEDRIVE_DOWNLOAD_FILE"),
        ],
    ),
    composio(
        "outlook",
        "Outlook",
        "Email, calendar and contacts",
        &[
            read("OUTLOOK_SEARCH_MESSAGES"),
            read("OUTLOOK_GET_MESSAGE"),
            read("OUTLOOK_DOWNLOAD_OUTLOOK_ATTACHMENT"),
            read("OUTLOOK_QUERY_EMAILS"),
            act("OUTLOOK_SEND_EMAIL"),
            act("OUTLOOK_REPLY_EMAIL"),
            act("OUTLOOK_MOVE_MESSAGE"),
            read("OUTLOOK_GET_CALENDAR_VIEW"),
            act("OUTLOOK_CALENDAR_CREATE_EVENT"),
            act("OUTLOOK_ACCEPT_EVENT"),
            act("OUTLOOK_DECLINE_EVENT"),
            act("OUTLOOK_UPDATE_CALENDAR_EVENT"),
            act("OUTLOOK_DELETE_CALENDAR_EVENT"),
        ],
    ),
    composio(
        "microsoft_teams",
        "Microsoft Teams",
        "Read and send messages",
        &[
            read("MICROSOFT_TEAMS_SEARCH_MESSAGES"),
            read("MICROSOFT_TEAMS_CHATS_GET_ALL_CHATS"),
            read("MICROSOFT_TEAMS_GET_CHAT_MESSAGE"),
            read("MICROSOFT_TEAMS_TEAMS_LIST_CHANNEL_MESSAGES"),
            act("MICROSOFT_TEAMS_TEAMS_POST_CHAT_MESSAGE"),
        ],
    ),
    composio(
        "one_drive",
        "OneDrive",
        "Find and read your files",
        &[
            read("ONE_DRIVE_SEARCH_DRIVE_ITEMS"),
            read("ONE_DRIVE_GET_RECENT_ITEMS"),
            read("ONE_DRIVE_GET_SHARED_ITEMS"),
            read("ONE_DRIVE_GET_ITEM"),
            read("ONE_DRIVE_LIST_FOLDER_CHILDREN"),
            read("ONE_DRIVE_GET_DRIVE_ITEM_BY_SHARING_URL"),
            read("ONE_DRIVE_DOWNLOAD_FILE"),
            act("ONE_DRIVE_MOVE_ITEM"),
            act("ONE_DRIVE_ONEDRIVE_CREATE_FOLDER"),
            act("ONE_DRIVE_CREATE_LINK"),
        ],
    ),
    composio(
        "excel",
        "Excel",
        "Read and edit workbooks",
        &[
            read("EXCEL_SEARCH_FILES"),
            read("EXCEL_GET_WORKSHEET_USED_RANGE"),
            read("EXCEL_LIST_TABLE_COLUMNS"),
            act("EXCEL_INSERT_RANGE"),
        ],
    ),
    composio(
        SLACK,
        "Slack",
        "Read and send messages",
        &[
            read("SLACK_SEARCH_MESSAGES"),
            read("SLACK_FETCH_CONVERSATION_HISTORY"),
            read("SLACK_FETCH_MESSAGE_THREAD_FROM_A_CONVERSATION"),
            read("SLACK_LIST_UNREAD_CHANNEL_MESSAGES"),
            read("SLACK_FIND_CHANNELS"),
            read("SLACK_FIND_USERS"),
            // Only opens the DM channel a message to a person needs, so the
            // approval comes once, at the send. Composio tags it as creating.
            read_over_hints("SLACK_OPEN_DM"),
            act("SLACK_SEND_MESSAGE"),
            act("SLACK_ADD_REACTION_TO_AN_ITEM"),
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
    /// Where Composio sends the browser after OAuth, with `status` and `connected_account_id` appended.
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
                    tool.slug.starts_with(&prefix),
                    "{} is not a {} tool",
                    tool.slug,
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
