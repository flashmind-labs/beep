//! The MCP servers the user saved and their credentials.

use super::User;
use crate::{
    apps::permissions::Rules,
    store::{AppPermissionRow, McpServerRow},
};
use anyhow::Result;
use flashmind::types::{Model, ReasoningLevel};
use serde_json::Value;

impl User {
    /// The user's server-managed MCP servers.
    pub async fn mcp_servers(&self) -> Result<Vec<McpServerRow>> {
        self.store.mcp_servers(&self.key).await
    }

    /// Saves a server's config; credentials are untouched, so it cannot roll back a rotated token.
    pub async fn save_mcp_config(&self, name: &str, config: Value) -> Result<()> {
        self.store.save_mcp_config(&self.key, name, config).await
    }

    /// Saves a server's OAuth credentials; `Value::Null` clears them.
    pub async fn save_mcp_credentials(&self, name: &str, credentials: Value) -> Result<()> {
        self.store
            .save_mcp_credentials(&self.key, name, credentials)
            .await
    }

    /// What Beep may do in each app.
    pub(crate) async fn app_rules(&self) -> Result<Rules> {
        Ok(Rules::from_rows(
            self.store.app_permissions(&self.key).await?,
        ))
    }

    /// The model the user picked, if any.
    pub(crate) async fn model(&self) -> Result<Option<String>> {
        self.store.user_model(&self.key).await
    }

    pub(crate) async fn set_model(&self, model: &Model) -> Result<()> {
        self.store
            .set_user_model(&self.key, Some(&model.to_string()))
            .await
    }

    /// The reasoning level the user picked, if any.
    pub(crate) async fn thinking(&self) -> Result<Option<ReasoningLevel>> {
        let picked = self.store.user_thinking(&self.key).await?;
        Ok(picked.and_then(|level| level.parse().ok()))
    }

    pub(crate) async fn set_thinking(&self, level: ReasoningLevel) -> Result<()> {
        self.store
            .set_user_thinking(&self.key, &level.to_string())
            .await
    }

    /// Whether Jev picks the model and thinking per prompt.
    pub(crate) async fn auto_model(&self) -> Result<bool> {
        self.store.user_auto_model(&self.key).await
    }

    pub(crate) async fn set_auto_model(&self, enabled: bool) -> Result<()> {
        self.store.set_user_auto_model(&self.key, enabled).await
    }

    pub(crate) async fn save_app_permissions(&self, row: &AppPermissionRow) -> Result<()> {
        self.store.save_app_permissions(&self.key, row).await
    }

    /// Deletes a saved MCP server; `false` when the user had none by that name.
    pub async fn delete_mcp_server(&self, name: &str) -> Result<bool> {
        self.store.delete_mcp_server(&self.key, name).await
    }
}
