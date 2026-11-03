//! A user's flsh settings, one row per user, the same on every machine.

use super::Store;
use crate::{protocol::FlshSettings, user::UserKey};
use anyhow::{Context, Result};
use sqlx::types::Json;

impl Store {
    /// The user's flsh settings; `None` until they save some.
    pub(crate) async fn flsh_settings(&self, user: &UserKey) -> Result<Option<FlshSettings>> {
        let row: Option<Json<FlshSettings>> =
            sqlx::query_scalar("SELECT settings FROM flsh_settings WHERE user_id=$1")
                .bind(user.as_str())
                .fetch_optional(&self.pool)
                .await
                .context("the flsh settings do not read")?;
        Ok(row.map(|Json(settings)| settings))
    }

    /// Saves the user's flsh settings in place of the ones before.
    pub(crate) async fn save_flsh_settings(
        &self,
        user: &UserKey,
        settings: &FlshSettings,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO flsh_settings (user_id, settings) VALUES ($1, $2)
             ON CONFLICT (user_id) DO UPDATE SET settings=$2, updated_at=now()",
        )
        .bind(user.as_str())
        .bind(Json(settings))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
