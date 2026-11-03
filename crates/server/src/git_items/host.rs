//! One call to a git host's API with a user's token, and why it failed in
//! words for the user.

use reqwest::{Client, StatusCode, header, header::HeaderMap};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;

/// Why a host's items are missing. The text never carries the host's own
/// reply: a Forgejo URL is the user's to type, so it could be anything.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostError {
    #[error("{0} refused the token. Check that it is current and can read your repositories.")]
    Refused(String),
    #[error("{0} says the token has made too many requests. Beep tries again in a few minutes.")]
    RateLimited(String),
    #[error("{0} did not answer in time.")]
    Timeout(String),
    #[error("Could not reach {0}.")]
    Unreachable(String),
    #[error("{0} is on your own network, which Beep's server cannot reach.")]
    Private(String),
    #[error("{host} answered with an error ({status}).")]
    Status { host: String, status: u16 },
    #[error("{0} sent a reply Beep could not read.")]
    BadReply(String),
    #[error("Beep could not ask {0}.")]
    Internal(String),
}

/// What a call needs: the client, the server-wide limit on calls in flight,
/// the host's name for messages and the token.
pub struct Caller<'a> {
    pub client: &'a Client,
    pub calls: &'a Arc<Semaphore>,
    pub host: &'a str,
    pub token: &'a SecretString,
}

impl Caller<'_> {
    /// `GET url` with the token, read as JSON.
    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, HostError> {
        let host = self.host.to_owned();
        let _permit = self
            .calls
            .acquire()
            .await
            .map_err(|_| HostError::Internal(host.clone()))?;
        let sent = self
            .client
            .get(url)
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("token {}", self.token.expose_secret()),
            )
            .send()
            .await;
        let reply = sent.map_err(|e| send_error(&host, &e))?;
        let status = reply.status();
        if !status.is_success() {
            return Err(status_error(host, status, reply.headers()));
        }
        reply.json::<T>().await.map_err(|e| {
            log::info!("reading open items from {host}: {e}");
            HostError::BadReply(host)
        })
    }
}

/// Why a call that got no reply failed.
pub(crate) fn send_error(host: &str, error: &reqwest::Error) -> HostError {
    if error.is_timeout() {
        return HostError::Timeout(host.to_owned());
    }
    log::info!("asking {host}: {error}");
    HostError::Unreachable(host.to_owned())
}

/// Why a reply with the unsuccessful `status` failed.
pub(crate) fn status_error(host: String, status: StatusCode, headers: &HeaderMap) -> HostError {
    let out_of_calls = headers
        .get("x-ratelimit-remaining")
        .is_some_and(|left| left.as_bytes() == b"0");
    match status {
        StatusCode::TOO_MANY_REQUESTS => HostError::RateLimited(host),
        StatusCode::FORBIDDEN if out_of_calls => HostError::RateLimited(host),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => HostError::Refused(host),
        _ => HostError::Status {
            host,
            status: status.as_u16(),
        },
    }
}
