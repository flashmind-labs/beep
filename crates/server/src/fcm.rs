//! Sends notices through Firebase Cloud Messaging when the server cannot
//! reach a phone over iroh. Each notice is sealed to the phone's endpoint
//! id, so Google carries it without reading it. A notice too big for a push
//! waits on the server, and an empty push wakes the phone to fetch it over
//! iroh.

use crate::{
    AsyncMutex,
    app::App,
    protocol::{Notice, seal},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use iroh::EndpointId;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
/// Fetch a new access token this long before the old one runs out.
const REFRESH_EARLY: Duration = Duration::from_secs(5 * 60);
/// FCM takes 4096 bytes of data; this leaves room for the field name.
const MAX_SEALED: usize = 4000;

/// Reaches a phone the server cannot reach over iroh. `Firebase` in
/// production, a recorder in tests.
#[async_trait]
pub trait Fallback: Send + Sync + 'static {
    /// Sends `notice` to the phone. True when the push service took it.
    async fn push(&self, phone: EndpointId, notice: &Notice) -> bool;
    /// Wakes the phone to connect and take its queued notices.
    async fn wake(&self, phone: EndpointId);
}

/// What a push to one token came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Sent,
    /// The app was uninstalled or the token rotated: forget it.
    Gone,
    Failed,
}

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

/// Sends data-only pushes through the FCM HTTP v1 API to the tokens the
/// user's phones registered.
pub struct Firebase {
    app: Arc<App>,
    client: Client,
    account: ServiceAccount,
    key: EncodingKey,
    access: AsyncMutex<Option<AccessToken>>,
}

impl Firebase {
    /// Reads a service account key file downloaded from the Google Cloud console.
    pub fn new(app: Arc<App>, key_file: &Path) -> Result<Self> {
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
            app,
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

/// Whether a 400 body blames the token itself, as for the placeholder early
/// builds registered. A 400 about the rest of the message keeps the token.
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
    /// Sends a push to each phone that registered with `phone`'s key. True
    /// when one took it.
    async fn send_all(&self, phone: EndpointId, android: Value, data: Value) -> bool {
        let store = &self.app.store;
        let tokens = match store.push_tokens(&phone).await {
            Ok(tokens) => tokens,
            Err(e) => {
                log::warn!("fcm: reading tokens failed; user={phone} error={e:#}");
                return false;
            }
        };
        if tokens.is_empty() {
            log::debug!("fcm: no tokens to push to; user={phone}");
        }
        let mut sent = false;
        // The token itself is never logged: it is all anyone needs to push
        // to that phone.
        for token in tokens {
            match self.send(&token, &android, &data).await {
                Ok(Delivery::Sent) => sent = true,
                Ok(Delivery::Gone) => {
                    if let Err(e) = store.forget_push_token(&phone, &token).await {
                        log::warn!("fcm: forgetting a token failed; user={phone} error={e:#}");
                    }
                }
                Ok(Delivery::Failed) => {}
                Err(e) => log::warn!("fcm: push failed; user={phone} error={e:#}"),
            }
        }
        sent
    }
}

/// `notice` sealed to `phone`, its body cut short to fit in a push. `None`
/// when it does not fit even without a body.
fn seal_to_fit(phone: &EndpointId, notice: &Notice) -> Result<Option<String>> {
    let mut notice = notice.clone();
    loop {
        let sealed = seal::seal(phone, &notice)?;
        if sealed.len() <= MAX_SEALED {
            return Ok(Some(sealed));
        }
        if notice.body.is_empty() {
            return Ok(None);
        }
        // Base64 grows the bytes by a third; cut that much and a little more.
        let over = (sealed.len() - MAX_SEALED) * 3 / 4 + 16;
        let mut keep = notice.body.len().saturating_sub(over);
        while !notice.body.is_char_boundary(keep) {
            keep -= 1;
        }
        notice.body.truncate(keep);
        notice.body.truncate(notice.body.trim_end().len());
        if !notice.body.is_empty() {
            notice.body.push_str("...");
        }
    }
}

#[async_trait]
impl Fallback for Firebase {
    async fn push(&self, phone: EndpointId, notice: &Notice) -> bool {
        let sealed = match seal_to_fit(&phone, notice) {
            Ok(Some(sealed)) => sealed,
            Ok(None) => return false,
            Err(e) => {
                log::warn!("fcm: sealing a notice failed; user={phone} error={e:#}");
                return false;
            }
        };
        // High priority reaches the phone in Doze. A changed chat shows
        // nothing, and Android demotes apps whose urgent pushes show nothing.
        let priority = if notice.changed { "NORMAL" } else { "HIGH" };
        let android = json!({ "priority": priority, "ttl": "86400s" });
        let sent = self
            .send_all(phone, android, json!({ "notice": sealed }))
            .await;
        if sent {
            log::info!("fcm: pushed a notice; user={phone}");
        }
        sent
    }

    async fn wake(&self, phone: EndpointId) {
        // Repeated wakes collapse into one.
        let android = json!({ "priority": "HIGH", "ttl": "600s", "collapse_key": "wake" });
        if self.send_all(phone, android, json!({ "wake": "1" })).await {
            log::info!("fcm: woke phone; user={phone}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn notice(body: String) -> Notice {
        Notice {
            title: "Done".into(),
            body,
            session_id: "s1".into(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        }
    }

    #[test]
    fn a_long_body_is_cut_to_fit_a_push() {
        let phone = SecretKey::from_bytes(&[1; 32]);
        let long = "é".repeat(5000);
        let sealed = seal_to_fit(&phone.public(), &notice(long.clone()))
            .unwrap()
            .expect("a body can always be cut");
        assert!(sealed.len() <= MAX_SEALED);
        let opened = seal::open(&phone, &sealed).unwrap();
        assert!(opened.body.ends_with("..."));
        assert!(long.starts_with(opened.body.trim_end_matches("...")));
        assert!(opened.body.len() > 2000);
    }

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

    #[test]
    fn a_short_body_is_kept_whole() {
        let phone = SecretKey::from_bytes(&[1; 32]);
        let sealed = seal_to_fit(&phone.public(), &notice("hi".into()))
            .unwrap()
            .unwrap();
        assert_eq!(seal::open(&phone, &sealed).unwrap().body, "hi");
    }
}
