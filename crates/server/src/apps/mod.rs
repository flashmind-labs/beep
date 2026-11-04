//! The curated app list, its Composio connections and the OAuth MCP servers.
//! Errors reaching the client never carry provider bodies or keys.

mod catalog;
mod composio;
pub(crate) use catalog::{CATALOG, CatalogApp, catalog_app, read};
pub use composio::{Composio, ComposioSettings};
pub(crate) use composio::{
    TOOLS_TIMEOUT, connect_url, connected_accounts, connected_slugs, internal, mcp_app,
};

pub mod feeds;
pub mod grants;
pub mod permissions;

pub(crate) mod custom_mcp;
pub(crate) mod files;
pub(crate) mod markdown;
pub mod mcp;

use crate::apps::permissions::Acts;
