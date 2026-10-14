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
    /// Set for a server the user added by URL.
    pub custom: Option<CustomMcp>,
}

/// What the user gave for a server they added, and whether it signs in.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomMcp {
    pub url: String,
    pub label: String,
    pub sign_in: bool,
}

impl Store {
    /// The user's server-managed MCP servers.
    pub(crate) async fn mcp_servers(&self, user: &UserKey) -> Result<Vec<McpServerRow>> {
        let rows = sqlx::query(
            "SELECT name, config, credentials, url, label, sign_in FROM mcp_servers
             WHERE user_id=$1 ORDER BY created_at, name",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                let url: Option<String> = r.try_get(3)?;
                let label: Option<String> = r.try_get(4)?;
                Ok(McpServerRow {
                    name: r.try_get(0)?,
                    config: r.try_get(1)?,
                    credentials: r.try_get(2)?,
                    custom: url.map(|url| CustomMcp {
                        label: label.unwrap_or_else(|| url.clone()),
                        url,
                        sign_in: r.try_get(5).unwrap_or(false),
                    }),
                })
            })
            .collect()
    }

    /// Adds a server the user gave by URL, or changes its name and URL.
    /// Returns false when `name` is already taken by a server that is not
    /// the user's own addition, such as Fastmail.
    pub(crate) async fn add_custom_mcp(
        &self,
        user: &UserKey,
        name: &str,
        custom: &CustomMcp,
    ) -> Result<bool> {
        let added = sqlx::query(
            "INSERT INTO mcp_servers (user_id, name, url, label, sign_in) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, name) DO UPDATE SET url=excluded.url, label=excluded.label,
                 sign_in=excluded.sign_in
             WHERE mcp_servers.url IS NOT NULL",
        )
        .bind(user.as_str())
        .bind(name)
        .bind(&custom.url)
        .bind(&custom.label)
        .bind(custom.sign_in)
        .execute(&self.pool)
        .await?;
        Ok(added.rows_affected() > 0)
    }

    /// Records that an added server asked to sign in.
    pub(crate) async fn set_mcp_sign_in(&self, user: &UserKey, name: &str) -> Result<()> {
        sqlx::query(
            "UPDATE mcp_servers SET sign_in = TRUE WHERE user_id=$1 AND name=$2 AND url IS NOT NULL",
        )
        .bind(user.as_str())
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(())
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
