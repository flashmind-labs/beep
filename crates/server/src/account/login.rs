//! Hosted login. The website signs the user in and gives the app a signed token, which the app
//! hands back here, signed in as the key that asked for the code. A code only works for
//! its own key, so a login link sent to someone else cannot attach another person's device.
//!
//! Token: `<payload>.<signature>`, both unpadded base64url. The signature is Ed25519 over the
//! payload's characters; the payload is JSON, see `Claims`.

use crate::{
    app::App,
    protocol::{ApiError, SecretText, link::MAX_NAME},
    store::{Login, LoginEnd, NewKey},
};
use anyhow::Context;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use iroh::{EndpointId, PublicKey, SecretKey, Signature};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

/// How long a code stays good.
const CODE_TTL: Duration = Duration::from_secs(10 * 60);
/// The token version this server reads.
const TOKEN_VERSION: u32 = 1;

/// The config's `[login]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginConfig {
    /// The website, such as `https://example.com`.
    pub website: Url,
    /// The website's Ed25519 public key: 32 bytes as hex.
    pub website_key: PublicKey,
}

/// What a token says. No `Debug`: `code` must not reach a log.
#[derive(Deserialize)]
struct Claims {
    v: u32,
    code: String,
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    exp: u64,
}

/// A new code for `endpoint`, as the website address that takes it.
pub(crate) async fn start(
    app: &App,
    endpoint: &EndpointId,
    device: &str,
) -> Result<SecretText, ApiError> {
    let config = config(app)?;
    if device.trim().is_empty() || device.len() > MAX_NAME {
        return Err(ApiError::bad_request(
            "the device name is empty or too long",
        ));
    }
    // A secret key's 32 bytes are good random ones.
    let code = URL_SAFE_NO_PAD.encode(SecretKey::generate().to_bytes());
    app.store
        .add_login_code(&code_hash(&code), endpoint, Utc::now() + CODE_TTL)
        .await?;
    Ok(SecretText::new(login_url(&config.website, &code, device)?))
}

/// Checks `token` and signs `key` in as its login. An account already holding `key` gets the login
/// attached instead.
pub(crate) async fn complete(app: &App, key: NewKey<'_>, token: &str) -> Result<(), ApiError> {
    let config = config(app)?;
    let claims = read_token(token, &config.website_key, SystemTime::now()).ok_or_else(refused)?;
    let login = Login {
        sub: &claims.sub,
        email: claims.email.as_deref(),
        name: claims.name.as_deref(),
    };
    let endpoint = key.endpoint;
    match app
        .store
        .complete_login(&code_hash(&claims.code), &login, key)
        .await?
    {
        LoginEnd::SignedIn { user } => {
            log::info!("a login signed in; user={user} key={endpoint}");
            if let Some(user) = app.user(&user) {
                user.changed("");
            }
            Ok(())
        }
        LoginEnd::BadCode => Err(refused()),
        LoginEnd::Taken => Err(ApiError::Conflict {
            message: "that login is on another account".into(),
        }),
    }
}

fn config(app: &App) -> Result<&LoginConfig, ApiError> {
    app.login()
        .ok_or_else(|| ApiError::bad_request("this server has no sign-in"))
}

/// Every token or code that fails says the same, naming neither.
fn refused() -> ApiError {
    ApiError::bad_request("that sign-in expired or was for another device")
}

fn code_hash(code: &str) -> [u8; 32] {
    *blake3::hash(code.as_bytes()).as_bytes()
}

fn login_url(website: &Url, code: &str, device: &str) -> anyhow::Result<String> {
    let mut url = website.clone();
    url.path_segments_mut()
        .ok()
        .context("the login website is not a web address")?
        .pop_if_empty()
        .push("login");
    url.query_pairs_mut()
        .append_pair("code", code)
        .append_pair("device", device);
    Ok(url.into())
}

/// What `token` says, when `key` signed it and it is current; `None` otherwise.
fn read_token(token: &str, key: &PublicKey, now: SystemTime) -> Option<Claims> {
    let (payload, signature) = token.trim().split_once('.')?;
    let signature: [u8; 64] = URL_SAFE_NO_PAD.decode(signature).ok()?.try_into().ok()?;
    key.verify(payload.as_bytes(), &Signature::from_bytes(&signature))
        .ok()?;
    let claims: Claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    let now = now.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let current = claims.v == TOKEN_VERSION && claims.exp > now && !claims.sub.is_empty();
    current.then_some(claims)
}
