//! Per-user app settings: server-managed MCP servers and app permissions.

use super::Store;
use crate::{protocol::PermissionLevel, user::UserKey};
use anyhow::Result;
use serde_json::Value;
use sqlx::Row;

/// One row of `app_permissions`.
pub(crate) struct AppPermissionRow {
    pub(crate) app: String,
    pub(crate) level: PermissionLevel,
    pub(crate) allowed: Vec<String>,
}

/// One row of `mcp_servers`.
pub struct McpServerRow {
    pub name: String,
    pub config: Value,
    pub credentials: Option<Value>,
}

impl Store {
    /// The user's server-managed MCP servers.
    pub(crate) async fn mcp_servers(&self, user: &UserKey) -> Result<Vec<McpServerRow>> {
        let rows = sqlx::query(
            "SELECT name, config, credentials FROM mcp_servers WHERE user_id=$1 ORDER BY name",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(McpServerRow {
                    name: r.try_get(0)?,
                    config: r.try_get(1)?,
                    credentials: r.try_get(2)?,
                })
            })
            .collect()
    }

    /// Saves a server's config. Credentials are untouched: they change only
    /// through `save_mcp_credentials`, so a stale config cannot roll back a
    /// rotated refresh token.
    pub(crate) async fn save_mcp_config(
        &self,
        user: &UserKey,
        name: &str,
        config: Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO mcp_servers (user_id, name, config) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, name) DO UPDATE SET config=excluded.config",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(config)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Saves a server's OAuth credentials; `Value::Null` clears them.
    pub(crate) async fn save_mcp_credentials(
        &self,
        user: &UserKey,
        name: &str,
        credentials: Value,
    ) -> Result<()> {
        let credentials = (!credentials.is_null()).then_some(credentials);
        sqlx::query(
            "INSERT INTO mcp_servers (user_id, name, credentials) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, name) DO UPDATE SET credentials=excluded.credentials",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(credentials)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every app the user set permissions for. A level this build does not
    /// know reads as `Ask`.
    pub(crate) async fn app_permissions(&self, user: &UserKey) -> Result<Vec<AppPermissionRow>> {
        let rows = sqlx::query("SELECT app, level, allowed FROM app_permissions WHERE user_id=$1")
            .bind(user.as_str())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|r| {
                Ok(AppPermissionRow {
                    app: r.try_get(0)?,
                    level: PermissionLevel::parse(r.try_get(1)?).unwrap_or(PermissionLevel::Ask {}),
                    allowed: r.try_get(2)?,
                })
            })
            .collect()
    }

    pub(crate) async fn save_app_permissions(
        &self,
        user: &UserKey,
        row: &AppPermissionRow,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_permissions (user_id, app, level, allowed) VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, app) DO UPDATE SET level=excluded.level, allowed=excluded.allowed",
        )
        .bind(user.as_str())
        .bind(&row.app)
        .bind(row.level.as_str())
        .bind(&row.allowed)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_mcp_server(&self, user: &UserKey, name: &str) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM mcp_servers WHERE user_id=$1 AND name=$2")
            .bind(user.as_str())
            .bind(name)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }
}
