//! Phones registered for push; a token belongs to the key that registered it, since notices are sealed to that key.

use super::Store;
use crate::user::UserKey;
use anyhow::{Context, Result};
use iroh::EndpointId;
use strum::{EnumString, IntoStaticStr};

/// The service that delivers to a token, as stored in `devices.service`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PushService {
    Fcm,
    ApnsSandbox,
    ApnsProduction,
}

/// A phone registering for push, over the connection of `endpoint`.
pub struct PushDevice<'a> {
    pub endpoint: EndpointId,
    pub device_id: &'a str,
    pub token: &'a str,
    pub service: PushService,
}

/// A token one of a key's phones registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushToken {
    pub token: String,
    pub service: PushService,
}

impl Store {
    pub(crate) async fn register_device(
        &self,
        user: &UserKey,
        device: PushDevice<'_>,
    ) -> Result<()> {
        let service: &str = device.service.into();
        sqlx::query(
            "INSERT INTO devices (user_id, device_id, fcm_token, endpoint_id, service) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, device_id) DO UPDATE SET fcm_token=excluded.fcm_token,
                 endpoint_id=excluded.endpoint_id, service=excluded.service, created_at=now()",
        )
        .bind(user.as_str())
        .bind(device.device_id)
        .bind(device.token)
        .bind(device.endpoint.to_string())
        .bind(service)
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
    pub(crate) async fn push_tokens(&self, endpoint: &EndpointId) -> Result<Vec<PushToken>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT fcm_token, service FROM devices WHERE endpoint_id=$1")
                .bind(endpoint.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|(token, service)| {
                let service = service
                    .parse()
                    .with_context(|| format!("unknown push service {service}"))?;
                Ok(PushToken { token, service })
            })
            .collect()
    }

    /// Drops `token` from whichever of `endpoint`'s phones had it.
    pub(crate) async fn forget_push_token(&self, endpoint: &EndpointId, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM devices WHERE endpoint_id=$1 AND fcm_token=$2")
            .bind(endpoint.to_string())
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
