//! Settings an admin changes for the whole server.

use super::Store;
use anyhow::Result;
use serde_json::Value;

/// The `server_settings` key that turns agents on.
const AGENTS: &str = "agents";

impl Store {
    /// Whether an admin turned agents on; `default` until one changes it.
    pub(crate) async fn agents_enabled(&self, default: bool) -> Result<bool> {
        let value: Option<Value> =
            sqlx::query_scalar("SELECT value FROM server_settings WHERE key = $1")
                .bind(AGENTS)
                .fetch_optional(&self.pool)
                .await?;
        Ok(value.and_then(|value| value.as_bool()).unwrap_or(default))
    }

    /// Turns agents on or off for everyone, recording which admin did.
    pub async fn set_agents_enabled(&self, enabled: bool, by: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO server_settings (key, value, updated_by) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE
             SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = now()",
        )
        .bind(AGENTS)
        .bind(Value::Bool(enabled))
        .bind(by)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
