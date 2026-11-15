//! Posts a report zip to the server, signed by the device key.

use beep_proto::ApiError;
use beep_proto::report::{
    DEFAULT_SERVER, KEY_HEADER, PATH, Receipt, SIGNATURE_HEADER, TIME_HEADER, signed_message,
};
use iroh::SecretKey;
use reqwest::header::CONTENT_TYPE;
use std::env::{self, VarError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use url::{ParseError, Url};

#[derive(Debug, Error)]
pub enum SendError {
    /// The server's `ApiError` text.
    #[error("{0}")]
    Refused(String),
    #[error("could not reach Beep: {0}")]
    Net(#[from] reqwest::Error),
    /// A reply that is not Beep's: nothing answers reports at that address yet.
    #[error("Problem reports are not available yet.")]
    Unavailable,
    #[error("BEEP_REPORT_URL is not a URL: {0}")]
    Url(String),
}

/// How long a report may take to post, the server's answer included: this
/// long, and as long again for each `SLOWEST` bytes of it.
const SEND_LIMIT: Duration = Duration::from_secs(60);
/// The bytes a second of the slowest upload worth waiting for carries.
const SLOWEST: usize = 64 * 1024;

/// Where reports go: `BEEP_REPORT_URL` when set, else the server the build
/// names in `BEEP_SERVER_URL`, else the Beep server.
pub fn server_url() -> Result<Url, SendError> {
    let url = match env::var("BEEP_REPORT_URL") {
        Ok(url) => url,
        Err(VarError::NotPresent) => {
            // A build without the variable, or with it empty, leaves the default.
            option_env!("BEEP_SERVER_URL")
                .filter(|url| !url.is_empty())
                .unwrap_or(DEFAULT_SERVER)
                .to_owned()
        }
        Err(e) => return Err(SendError::Url(e.to_string())),
    };
    Url::parse(&url).map_err(|e: ParseError| SendError::Url(e.to_string()))
}

/// Posts `body` to `base` + `/v1/reports`, signed by `key`.
pub async fn send(base: &Url, body: Vec<u8>, key: &SecretKey) -> Result<Receipt, SendError> {
    // A clock before 1970 signs time 0, which the server refuses as off.
    let at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let signature = key.sign(&signed_message(&body, at_ms));
    let url = format!("{}{PATH}", base.as_str().trim_end_matches('/'));
    let reply = beep_proto::https::client()
        .timeout(SEND_LIMIT + Duration::from_secs((body.len() / SLOWEST) as u64))
        .build()?
        .post(url)
        .header(CONTENT_TYPE, "application/zip")
        .header(KEY_HEADER, key.public().to_string())
        .header(TIME_HEADER, at_ms.to_string())
        .header(SIGNATURE_HEADER, hex::encode(signature.to_bytes()))
        .body(body)
        .send()
        .await?;
    let status = reply.status();
    let bytes = reply.bytes().await?;

    if status.is_success() {
        return rmp_serde::from_slice(&bytes).map_err(|_| SendError::Unavailable);
    }

    match rmp_serde::from_slice::<ApiError>(&bytes) {
        Ok(ApiError::BadRequest { message } | ApiError::Conflict { message }) => {
            Err(SendError::Refused(message))
        }
        Ok(other) => Err(SendError::Refused(other.to_string())),
        Err(_) => Err(SendError::Unavailable),
    }
}

#[cfg(test)]
#[path = "tests/send.rs"]
mod tests;
