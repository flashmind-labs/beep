//! The phones registered for push. A token belongs to the key that
//! registered it, since each notice is sealed to that key.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use iroh::EndpointId;

/// A phone registering for push, over the connection of `endpoint`.
pub struct PushDevice<'a> {
    pub endpoint: EndpointId,
    pub device_id: &'a str,
    pub fcm_token: &'a str,
}

impl Store {
    pub(crate) async fn register_device(
        &self,
        user: &UserKey,
        device: PushDevice<'_>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO devices (user_id, device_id, fcm_token, endpoint_id) VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, device_id) DO UPDATE SET fcm_token=excluded.fcm_token,
                 endpoint_id=excluded.endpoint_id, created_at=now()",
        )
        .bind(user.as_str())
        .bind(device.device_id)
        .bind(device.fcm_token)
        .bind(device.endpoint.to_string())
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

    /// The push tokens registered by `endpoint`.
    pub(crate) async fn push_tokens(&self, endpoint: &EndpointId) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT fcm_token FROM devices WHERE endpoint_id=$1")
                .bind(endpoint.to_string())
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// Drops `fcm_token` from whichever of `endpoint`'s phones had it.
    pub(crate) async fn forget_push_token(
        &self,
        endpoint: &EndpointId,
        fcm_token: &str,
    ) -> Result<()> {
        sqlx::query("DELETE FROM devices WHERE endpoint_id=$1 AND fcm_token=$2")
            .bind(endpoint.to_string())
            .bind(fcm_token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
