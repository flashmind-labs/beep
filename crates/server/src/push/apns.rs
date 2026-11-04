//! Sends notices to iPhones through APNs, sealed to the phone and opened by the app's extension.

use crate::{
    AsyncMutex,
    config::ApnsConfig,
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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PRODUCTION: &str = "https://api.push.apple.com";
const SANDBOX: &str = "https://api.sandbox.push.apple.com";
/// Apple refuses tokens older than an hour and renewals more often than every 20 minutes.
const TOKEN_LIFETIME: Duration = Duration::from_secs(40 * 60);
/// APNs takes 4096 bytes of payload; this leaves room for the rest of it.
const MAX_SEALED: usize = 3600;
/// How long APNs keeps trying a phone that is off.
const EXPIRES_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// What the phone shows when its extension cannot open the notice.
const PLACEHOLDER_TITLE: &str = "Beep";
const PLACEHOLDER_BODY: &str = "New message";

/// Which APNs server a token belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Sandbox,
    Production,
}

impl Environment {
    fn host(self) -> &'static str {
        match self {
            Self::Sandbox => SANDBOX,
            Self::Production => PRODUCTION,
        }
    }
}

/// One push: its JSON and the headers APNs needs to route it.
#[derive(Debug, PartialEq)]
struct Push {
    payload: Value,
    background: bool,
}

impl Push {
    /// `notice` sealed to `phone`; `None` when it does not fit even without a body.
    fn notice(phone: &EndpointId, notice: &Notice) -> Result<Option<Self>> {
        if notice.changed || notice.title.is_empty() {
            return Ok(Some(Self::wake()));
        }
        let Some(sealed) = seal_to_fit(phone, notice, MAX_SEALED)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            payload: json!({
                "aps": {
                    "alert": { "title": PLACEHOLDER_TITLE, "body": PLACEHOLDER_BODY },
                    "sound": "default",
                    "mutable-content": 1,
                },
                "notice": sealed,
            }),
            background: false,
        }))
    }

    /// Wakes the app in the background to connect and take what is queued.
    fn wake() -> Self {
        Self {
            payload: json!({ "aps": { "content-available": 1 } }),
            background: true,
        }
    }
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    iat: u64,
}

#[derive(Deserialize)]
struct Refusal {
    reason: String,
}

struct ProviderToken {
    value: SecretString,
    renew_at: Instant,
}

/// Sends pushes to the APNs tokens iPhones registered.
pub struct Apns {
    client: Client,
    key: EncodingKey,
    key_id: String,
    team_id: String,
    topic: String,
    token: AsyncMutex<Option<ProviderToken>>,
}

impl Apns {
    pub fn new(config: &ApnsConfig) -> Result<Self> {
        let path = &config.key_file;
        let pem = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let key = EncodingKey::from_ec_pem(&pem)
            .with_context(|| format!("{} is not an APNs key", path.display()))?;
        log::info!(
            "apns: sending for {} with key {}",
            config.topic,
            config.key_id
        );
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .http2_adaptive_window(true)
                .build()?,
            key,
            key_id: config.key_id.clone(),
            team_id: config.team_id.clone(),
            topic: config.topic.clone(),
            token: AsyncMutex::default(),
        })
    }

    /// Sends `notice` to the iPhone with `token`, sealed to its key `phone`.
    pub async fn push(
        &self,
        phone: &EndpointId,
        token: &str,
        environment: Environment,
        notice: &Notice,
    ) -> Result<Delivery> {
        match Push::notice(phone, notice)? {
            Some(push) => self.send(token, environment, &push).await,
            None => Ok(Delivery::Failed),
        }
    }

    pub async fn wake(&self, token: &str, environment: Environment) -> Result<Delivery> {
        self.send(token, environment, &Push::wake()).await
    }

    async fn provider_token(&self) -> Result<SecretString> {
        let mut cached = self.token.lock().await;
        if let Some(token) = cached.as_ref().filter(|t| Instant::now() < t.renew_at) {
            return Ok(token.value.clone());
        }
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let claims = Claims {
            iss: &self.team_id,
            iat: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        };
        let value = SecretString::from(jsonwebtoken::encode(&header, &claims, &self.key)?);
        *cached = Some(ProviderToken {
            value: value.clone(),
            renew_at: Instant::now() + TOKEN_LIFETIME,
        });
        Ok(value)
    }

    async fn send(&self, token: &str, environment: Environment, push: &Push) -> Result<Delivery> {
        let auth = self.provider_token().await?;
        let expires = SystemTime::now().duration_since(UNIX_EPOCH)? + EXPIRES_AFTER;
        let (push_type, priority) = if push.background {
            ("background", "5")
        } else {
            ("alert", "10")
        };
        let response = self
            .client
            .post(format!("{}/3/device/{token}", environment.host()))
            .bearer_auth(auth.expose_secret())
            .header("apns-topic", &self.topic)
            .header("apns-push-type", push_type)
            .header("apns-priority", priority)
            .header("apns-expiration", expires.as_secs().to_string())
            .json(&push.payload)
            .send()
            .await?;
        let status = response.status();
        if status == StatusCode::OK {
            return Ok(Delivery::Sent);
        }
        // The body names the reason. It never repeats the device token, which
        // is all anyone needs to push to that phone, so neither does the log.
        let body = response.text().await.unwrap_or_default();
        let reason = serde_json::from_str::<Refusal>(&body)
            .map(|r| r.reason)
            .unwrap_or(body);
        Ok(match (status, reason.as_str()) {
            (StatusCode::GONE, _) | (_, "BadDeviceToken" | "Unregistered") => {
                log::info!("apns: token no longer registered: {reason}");
                Delivery::Gone
            }
            (_, "ExpiredProviderToken" | "InvalidProviderToken") => {
                log::warn!("apns: provider token refused: {reason}");
                let mut cached = self.token.lock().await;
                if cached
                    .as_ref()
                    .is_some_and(|t| t.value.expose_secret() == auth.expose_secret())
                {
                    *cached = None;
                }
                Delivery::Failed
            }
            _ => {
                log::warn!("apns: answered {status}: {reason}");
                Delivery::Failed
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::seal;
    use iroh::SecretKey;

    fn notice(title: &str, changed: bool) -> Notice {
        Notice {
            title: title.into(),
            body: "The report is ready.".into(),
            session_id: "s1".into(),
            app: None,
            approval: None,
            changed,
            group: false,
        }
    }

    #[test]
    fn a_notice_goes_sealed_behind_a_placeholder() {
        let phone = SecretKey::from_bytes(&[1; 32]);
        let push = Push::notice(&phone.public(), &notice("Done", false))
            .unwrap()
            .unwrap();
        assert!(!push.background);
        assert_eq!(push.payload["aps"]["mutable-content"], 1);
        assert_eq!(push.payload["aps"]["alert"]["body"], PLACEHOLDER_BODY);
        let text = push.payload.to_string();
        assert!(!text.contains("report"), "the notice's text is sealed");
        let sealed = push.payload["notice"].as_str().unwrap();
        assert_eq!(seal::open(&phone, sealed).unwrap(), notice("Done", false));
    }

    #[test]
    fn a_long_notice_still_fits_a_push() {
        let phone = SecretKey::from_bytes(&[1; 32]).public();
        let mut long = notice("Done", false);
        long.body = "é".repeat(5000);
        let push = Push::notice(&phone, &long).unwrap().unwrap();
        assert!(push.payload.to_string().len() <= 4096);
    }

    /// Checks a real key against Apple, named by the `BEEP_APNS_*` env vars.
    #[tokio::test]
    #[ignore = "talks to Apple"]
    async fn apple_takes_the_key() {
        let var = |name| std::env::var(name).unwrap_or_else(|_| panic!("{name} is set"));
        let apns = Apns::new(&ApnsConfig {
            key_file: var("BEEP_APNS_KEY_FILE").into(),
            key_id: var("BEEP_APNS_KEY_ID"),
            team_id: var("BEEP_APNS_TEAM_ID"),
            topic: var("BEEP_APNS_TOPIC"),
        })
        .unwrap();
        let token = "00".repeat(32);
        for environment in [Environment::Sandbox, Environment::Production] {
            let delivery = apns.wake(&token, environment).await.unwrap();
            assert_eq!(delivery, Delivery::Gone, "{environment:?}");
        }
    }
}
