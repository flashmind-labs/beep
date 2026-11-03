//! The app catalog as one user sees it: their Composio accounts and MCP servers.

use super::User;
use crate::{
    apps::{
        CATALOG, Composio, TOOLS_TIMEOUT, catalog_app, connect_url, connected_slugs, internal,
        mcp_app,
    },
    permissions::{self, ToolSlug},
    protocol::{ApiError, AppAccount, AppIntegration, AppKind, ConnectLink},
    store::Feed,
};
use anyhow::Result;
use flashmind::{
    tools::composio::{ComposioClient, make_composio_tool_wrappers},
    types::Tool,
};
use secrecy::ExposeSecret;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// The Composio tools of a user's connected apps.
#[derive(Default)]
pub struct AppTools {
    pub tools: Vec<Arc<dyn Tool>>,
    /// Slugs whose Composio tags say they create or destroy something.
    pub(crate) hinted: HashSet<ToolSlug>,
}

impl User {
    /// A Composio client acting for the user, if the server has a key.
    pub fn composio_client(&self) -> Option<Arc<ComposioClient>> {
        Some(self.client_for(self.composio.as_deref()?))
    }

    fn client_for(&self, composio: &Composio) -> Arc<ComposioClient> {
        let key = composio.settings.key.expose_secret();
        Arc::new(
            ComposioClient::new(key.to_owned(), None, None)
                .with_entity_id(self.external_id().to_owned()),
        )
    }

    /// Agent tools for the user's connected Composio toolkits; a failing Composio gives none.
    pub async fn app_tools(&self) -> AppTools {
        let Some(composio) = self.composio.as_deref() else {
            return AppTools::default();
        };
        let client = self.client_for(composio);
        let fetch = async {
            let connected = connected_slugs(&client, self.external_id()).await?;
            let slugs: Vec<&str> = connected
                .iter()
                .filter_map(|slug| catalog_app(slug))
                .flat_map(|app| app.tools.iter().map(|tool| tool.slug))
                .collect();
            composio.tools(&slugs).await
        };
        match tokio::time::timeout(TOOLS_TIMEOUT, fetch).await {
            Ok(Ok(defs)) => AppTools {
                hinted: defs
                    .iter()
                    .filter(|def| permissions::hinted_to_act(&def.tags))
                    .map(|def| ToolSlug::new(&def.slug))
                    .collect(),
                tools: make_composio_tool_wrappers(&client, &defs),
            },
            Ok(Err(error)) => {
                log::warn!("apps: unable to list Composio tools: {error:#}");
                AppTools::default()
            }
            Err(_) => {
                log::warn!("apps: listing Composio tools timed out");
                AppTools::default()
            }
        }
    }

    /// Whether a Composio app is connected; `None` without Composio or an answer.
    pub async fn composio_connected(&self) -> Option<bool> {
        let client = self.composio_client()?;
        match connected_slugs(&client, self.external_id()).await {
            Ok(slugs) => Some(!slugs.is_empty()),
            Err(error) => {
                log::warn!("apps: unable to list connected accounts: {error:#}");
                None
            }
        }
    }

    /// The user's connected feeds from Composio and MCP; an outage leaves its feeds out.
    pub async fn feeds(&self) -> Vec<Feed> {
        let mut connected = HashSet::new();
        if let Some(client) = self.composio_client() {
            match connected_slugs(&client, self.external_id()).await {
                Ok(slugs) => connected.extend(slugs),
                Err(error) => log::warn!("apps: unable to list connected accounts: {error:#}"),
            }
        }
        match self.mcp_connected().await {
            Ok(servers) => connected.extend(servers),
            Err(error) => log::warn!("apps: unable to list MCP servers: {error:#}"),
        }
        Feed::ALL
            .into_iter()
            .filter(|feed| connected.contains(feed.slug()))
            .collect()
    }

    /// The app catalog with what the user connected.
    pub async fn apps(&self) -> Result<Vec<AppIntegration>, ApiError> {
        // A Composio failure marks its apps unavailable instead of failing the list.
        let (connected, logos) = match (&self.composio, self.composio_client()) {
            (Some(composio), Some(client)) => {
                let (connected, logos) = tokio::join!(
                    connected_slugs(&client, self.external_id()),
                    composio.logos()
                );
                let connected = connected
                    .map_err(|error| {
                        log::warn!("apps: unable to list connected accounts: {error:#}")
                    })
                    .ok();
                (connected, logos)
            }
            _ => (None, HashMap::new()),
        };
        let composio = connected.is_some();
        let connected = connected.unwrap_or_default();
        let connected_mcp = self
            .mcp_connected()
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

    /// A link that connects a Composio app to the user.
    pub async fn connect_app(&self, slug: &str) -> Result<ConnectLink, ApiError> {
        let client = self.composio_app(slug)?;
        let callback = self
            .composio
            .as_ref()
            .and_then(|composio| composio.settings.callback_url.as_deref());
        let url = connect_url(&client, self.external_id(), slug, callback)
            .await
            .map_err(internal("start OAuth"))?;
        Ok(ConnectLink { url })
    }

    /// Starts OAuth for an MCP app, finished by `finish_app_auth`; `add` signs in another account.
    pub async fn start_app_auth(
        &self,
        slug: &str,
        redirect_uri: &str,
        add: bool,
    ) -> Result<ConnectLink, ApiError> {
        mcp_app(slug)?;
        let url = self.start_fastmail_auth(redirect_uri, add).await?;
        Ok(ConnectLink { url })
    }

    /// The connected accounts of an MCP app, the default first.
    pub async fn app_accounts(&self, slug: &str) -> Result<Vec<AppAccount>, ApiError> {
        mcp_app(slug)?;
        let accounts = self
            .list_fastmail_accounts()
            .await
            .map_err(internal("list MCP servers"))?;
        Ok(accounts
            .into_iter()
            .map(|account| AppAccount {
                id: account.name,
                address: account.address,
            })
            .collect())
    }

    /// Disconnects one account of an MCP app.
    pub async fn remove_app_account(&self, slug: &str, id: &str) -> Result<(), ApiError> {
        mcp_app(slug)?;
        self.remove_fastmail_account(id).await
    }

    /// Completes an MCP app login with the code the phone caught.
    pub async fn finish_app_auth(
        &self,
        slug: &str,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<(), ApiError> {
        mcp_app(slug)?;
        self.finish_fastmail_auth(code, state, issuer)
            .await
            .map_err(internal("finish MCP OAuth"))
    }

    /// Disconnects an app from the user.
    pub async fn disconnect_app(&self, slug: &str) -> Result<(), ApiError> {
        if catalog_app(slug).is_some_and(|app| app.kind == (AppKind::Fastmail {})) {
            let removed = self
                .remove_fastmail()
                .await
                .map_err(internal("remove MCP server"))?;
            return if removed {
                Ok(())
            } else {
                Err(ApiError::NotFound {})
            };
        }
        let user = self.external_id();
        let client = self.composio_app(slug)?;
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
    fn composio_app(&self, slug: &str) -> Result<Arc<ComposioClient>, ApiError> {
        let app = catalog_app(slug).ok_or_else(|| ApiError::bad_request("unknown app"))?;
        if app.kind == (AppKind::Fastmail {}) {
            return Err(ApiError::bad_request(
                "Fastmail connects through StartAppAuth with a localhost redirect.",
            ));
        }
        self.composio_client().ok_or_else(|| {
            ApiError::bad_request("App connections are not configured on this server.")
        })
    }
}
