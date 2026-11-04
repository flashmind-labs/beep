//! Sends notices to Android phones through FCM, sealed to the phone so Google cannot read them.

use crate::{
    AsyncMutex,
    protocol::Notice,
    push::remote::{Delivery, seal_to_fit},
};
use anyhow::{Context, Result};
use iroh::EndpointId;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
/// Fetch a new access token this long before the old one runs out.
const REFRESH_EARLY: Duration = Duration::from_secs(5 * 60);
/// FCM takes 4096 bytes of data; this leaves room for the field name.
const MAX_SEALED: usize = 4000;

/// The fields used from a Google service account key file.
#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: SecretString,
    token_uri: String,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: SecretString,
    expires_in: u64,
}

struct AccessToken {
    value: SecretString,
    refresh_at: Instant,
}

/// Sends data-only pushes through the FCM HTTP v1 API.
pub struct Firebase {
    client: Client,
    account: ServiceAccount,
    key: EncodingKey,
    access: AsyncMutex<Option<AccessToken>>,
}

impl Firebase {
    /// Reads a service account key file downloaded from the Google Cloud console.
    pub fn new(key_file: &Path) -> Result<Self> {
        let data = fs::read(key_file).with_context(|| format!("reading {}", key_file.display()))?;
        let account: ServiceAccount = serde_json::from_slice(&data)
            .with_context(|| format!("{} is not a service account key", key_file.display()))?;
        let key = EncodingKey::from_rsa_pem(account.private_key.expose_secret().as_bytes())?;
        log::info!(
            "fcm: sending through project {} as {}",
            account.project_id,
            account.client_email
        );
        Ok(Self {
            client: Client::builder().timeout(Duration::from_secs(20)).build()?,
            account,
            key,
            access: AsyncMutex::default(),
        })
    }

    async fn access_token(&self) -> Result<SecretString> {
        let mut access = self.access.lock().await;
        if let Some(token) = access.as_ref().filter(|t| Instant::now() < t.refresh_at) {
            return Ok(token.value.clone());
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let claims = Claims {
            iss: &self.account.client_email,
            scope: SCOPE,
            aud: &self.account.token_uri,
            iat: now,
            exp: now + 3600,
        };
        let assertion = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.key)?;
        let response = self
            .client
            .post(&self.account.token_uri)
            .form(&[("grant_type", GRANT_TYPE), ("assertion", &assertion)])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            // Google's error body says what is wrong with the service account.
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("token request failed with {status}: {body}");
        }
        let token: TokenResponse = response.json().await?;
        log::debug!("fcm: access token valid for {}s", token.expires_in);
        let lifetime = Duration::from_secs(token.expires_in).saturating_sub(REFRESH_EARLY);
        let value = token.access_token.clone();
        *access = Some(AccessToken {
            value: token.access_token,
            refresh_at: Instant::now() + lifetime,
        });
        Ok(value)
    }

    async fn send(&self, token: &str, android: &Value, data: &Value) -> Result<Delivery> {
        let access = self.access_token().await?;
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.account.project_id
        );
        // Data-only, so Android hands it to the app to show, rather than
        // showing it itself.
        let message = json!({ "message": { "token": token, "data": data, "android": android } });
        let response = self
            .client
            .post(url)
            .bearer_auth(access.expose_secret())
            .json(&message)
            .send()
            .await?;
        let status = response.status();
        // The body names the message on success and the reason on failure.
        // It never repeats the device token.
        let body = response.text().await.unwrap_or_default();
        Ok(match status {
            StatusCode::OK => Delivery::Sent,
            // UNREGISTERED: the app is gone or got a new token. A 403 can be
            // this project's own setup, so it never drops tokens.
            StatusCode::NOT_FOUND => {
                log::info!("fcm: token no longer registered: {body}");
                Delivery::Gone
            }
            StatusCode::BAD_REQUEST if rejects_token(&body) => {
                log::info!("fcm: token is not a valid registration token");
                Delivery::Gone
            }
            // Google revoked the access token early: fetch a new one next time.
            StatusCode::UNAUTHORIZED => {
                log::warn!("fcm: access token refused: {body}");
                let mut cached = self.access.lock().await;
                if cached
                    .as_ref()
                    .is_some_and(|t| t.value.expose_secret() == access.expose_secret())
                {
                    *cached = None;
                }
                Delivery::Failed
            }
            _ => {
                log::warn!("fcm: answered {status}: {body}");
                Delivery::Failed
            }
        })
    }
}

/// Whether a 400 body blames the token itself rather than the message.
fn rejects_token(body: &str) -> bool {
    let Ok(body) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let details = body["error"]["details"].as_array().into_iter().flatten();
    details
        .filter_map(|detail| detail["fieldViolations"].as_array())
        .flatten()
        .any(|violation| violation["field"] == "message.token")
}

impl Firebase {
    /// Sends `notice` to the phone with `token`, sealed to its key `phone`.
    pub async fn push(&self, phone: &EndpointId, token: &str, notice: &Notice) -> Result<Delivery> {
        let Some(sealed) = seal_to_fit(phone, notice, MAX_SEALED)? else {
            return Ok(Delivery::Failed);
        };
        // High priority reaches the phone in Doze. A changed chat shows
        // nothing, and Android demotes apps whose urgent pushes show nothing.
        let priority = if notice.changed { "NORMAL" } else { "HIGH" };
        let android = json!({ "priority": priority, "ttl": "86400s" });
        self.send(token, &android, &json!({ "notice": sealed }))
            .await
    }

    /// Wakes the phone to connect and take its queued notices.
    pub async fn wake(&self, token: &str) -> Result<Delivery> {
        // Repeated wakes collapse into one.
        let android = json!({ "priority": "HIGH", "ttl": "600s", "collapse_key": "wake" });
        self.send(token, &android, &json!({ "wake": "1" })).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_rejected_token_is_dropped() {
        let violation = |field: &str| {
            json!({"error": {"code": 400, "details": [
                {"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError"},
                {"@type": "type.googleapis.com/google.rpc.BadRequest",
                 "fieldViolations": [{"field": field, "description": "invalid"}]},
            ]}})
            .to_string()
        };
        assert!(rejects_token(&violation("message.token")));
        assert!(!rejects_token(&violation("message.data")));
        assert!(!rejects_token("not json"));
    }
}
