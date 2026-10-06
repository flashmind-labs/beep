//! The phones registered for push.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;

impl Store {
    pub(crate) async fn register_device(
        &self,
        user: &UserKey,
        device_id: &str,
        fcm_token: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO devices (user_id, device_id, fcm_token) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, device_id) DO UPDATE SET fcm_token=excluded.fcm_token,
                 created_at=now()",
        )
        .bind(user.as_str())
        .bind(device_id)
        .bind(fcm_token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn unregister_device(&self, user: &UserKey, device_id: &str) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM devices WHERE user_id=$1 AND device_id=$2")
            .bind(user.as_str())
            .bind(device_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    pub(crate) async fn device_tokens(&self, user: &UserKey) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT fcm_token FROM devices WHERE user_id=$1")
                .bind(user.as_str())
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// Drops `fcm_token` from whichever of the user's phones had it.
    pub(crate) async fn forget_device_token(&self, user: &UserKey, fcm_token: &str) -> Result<()> {
        sqlx::query("DELETE FROM devices WHERE user_id=$1 AND fcm_token=$2")
            .bind(user.as_str())
            .bind(fcm_token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
