//! Checks a token with its host before it is saved, and learns who it
//! signs in as.

use super::{ForgejoUrl, net::PublicOnly};
use reqwest::{Client, StatusCode, header, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(15);
const GITHUB_USER: &str = "https://api.github.com/user";
const OPENROUTER_KEY: &str = "https://openrouter.ai/api/v1/key";

/// What a host said about a token.
#[derive(Debug, PartialEq, Eq)]
pub enum Checked {
    /// It signs in, as `login` when the host said.
    Valid { login: Option<String> },
    /// The host refused it; the text says why, for the user.
    Refused(String),
    /// The host could not be asked, such as a server only the user's own
    /// network reaches.
    Unreachable,
}

#[derive(Deserialize)]
struct UserReply {
    login: Option<String>,
}

/// Asks hosts about tokens.
pub struct Checker {
    client: Client,
}

impl Checker {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(TIMEOUT)
                .user_agent("beep-server")
                // A redirect could send the token, or the probe, anywhere.
                .redirect(Policy::none())
                .dns_resolver(Arc::new(PublicOnly))
                .build()?,
        })
    }

    /// `GET https://api.github.com/user` with `token`.
    pub async fn github(&self, token: &SecretString) -> Checked {
        self.user(GITHUB_USER, "GitHub", token).await
    }

    /// `GET <url>/api/v1/user` with `token`.
    pub async fn forgejo(&self, url: &ForgejoUrl, token: &SecretString) -> Checked {
        let host = url.host();
        self.user(&format!("{}/api/v1/user", url.as_str()), &host, token)
            .await
    }

    /// `GET https://openrouter.ai/api/v1/key`, which answers for a working key.
    pub async fn openrouter(&self, key: &SecretString) -> Checked {
        let sent = self
            .client
            .get(OPENROUTER_KEY)
            .bearer_auth(key.expose_secret())
            .send()
            .await;
        match sent {
            Ok(reply) if reply.status().is_success() => Checked::Valid { login: None },
            Ok(reply) => refused("OpenRouter", reply.status()),
            Err(e) => {
                log::info!("could not check an OpenRouter key: {e}");
                Checked::Unreachable
            }
        }
    }

    async fn user(&self, url: &str, host: &str, token: &SecretString) -> Checked {
        let sent = self
            .client
            .get(url)
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("token {}", token.expose_secret()),
            )
            .send()
            .await;
        let reply = match sent {
            Ok(reply) => reply,
            Err(e) => {
                log::info!("could not check a token with {host}: {e}");
                return Checked::Unreachable;
            }
        };
        if !reply.status().is_success() {
            return refused(host, reply.status());
        }
        match reply.json::<UserReply>().await {
            Ok(user) => Checked::Valid { login: user.login },
            Err(_) => Checked::Refused(format!(
                "{host} did not take the token. Check the URL and the token."
            )),
        }
    }
}

/// What the user is told about a refusal. It never carries the host's
/// status or body: the URL is the user's to type, so the reply could be
/// anything the server can reach.
fn refused(host: &str, status: StatusCode) -> Checked {
    Checked::Refused(match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            format!("{host} refused the token. Check that it is current and has access.")
        }
        _ => format!("{host} did not take the token. Check the URL and the token."),
    })
}
