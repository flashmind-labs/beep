//! One git host API call with a user's token, and its failure in plain words.

use anyhow::Result;
use beep_proto::{GitItem, GitItemKind, GitReview, ProjectIssue};
use reqwest::{Client, Response, StatusCode, header, header::HeaderMap};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{sync::Arc, time::SystemTime};
use thiserror::Error;
use tokio::sync::Semaphore;

/// What reaching a git host needs from where the code runs: a client and the limit on calls in
/// flight.
pub trait Hosts: Send + Sync {
    fn client(&self) -> Result<Client>;

    fn calls(&self) -> &Arc<Semaphore>;
}

/// A repository by host and `org/repo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Repo {
    pub host: String,
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
#[error("the item URL does not identify a host and repository")]
pub struct RepoUrlError;

impl TryFrom<&str> for Repo {
    type Error = RepoUrlError;

    fn try_from(url: &str) -> Result<Self, Self::Error> {
        let rest = url.split_once("://").ok_or(RepoUrlError)?.1;
        let mut parts = rest.split('/');
        let host = parts
            .next()
            .filter(|host| !host.is_empty())
            .ok_or(RepoUrlError)?;
        let name = format!(
            "{}/{}",
            parts.next().ok_or(RepoUrlError)?,
            parts.next().ok_or(RepoUrlError)?
        );
        Ok(Self {
            host: host.to_owned(),
            name,
        })
    }
}

/// Why a host's items are missing; never carries the host's own reply.
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
    #[error("{host} answered with an error ({status}).")]
    Status { host: String, status: u16 },
    #[error("{0} sent a reply Beep could not read.")]
    BadReply(String),
    #[error("Beep could not ask {0}.")]
    Internal(String),
}

/// An open item a host listed, with its label names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub item: GitItem,
    pub labels: Vec<String>,
    /// When the host says it was created. Not part of `GitItem`, whose bytes released apps read.
    pub opened_at: Option<SystemTime>,
}

impl TryFrom<&ProjectIssue> for Found {
    type Error = RepoUrlError;

    fn try_from(issue: &ProjectIssue) -> Result<Self, Self::Error> {
        let Repo { host, name: repo } = Repo::try_from(issue.url.as_str())?;
        Ok(Self {
            item: GitItem {
                host,
                repo,
                number: issue.number,
                kind: GitItemKind::Issue {},
                title: issue.title.clone(),
                url: issue.url.clone(),
                updated_at: issue.opened_at,
                authored: false,
                assigned: false,
                review_requested: false,
                draft: false,
                review: GitReview::None {},
                checks_failing: false,
                comments: issue.comments,
            },
            labels: issue.labels.clone(),
            opened_at: Some(issue.opened_at),
        })
    }
}

/// A label as GitHub and Forgejo give it on an issue.
#[derive(Debug, Deserialize)]
pub struct Label {
    pub name: String,
}

pub fn label_names(labels: &[Label]) -> Vec<String> {
    labels.iter().map(|label| label.name.clone()).collect()
}

/// What a call needs: client, call limit, host name and token.
pub struct Caller<'a> {
    pub client: &'a Client,
    pub calls: &'a Arc<Semaphore>,
    pub host: &'a str,
    pub token: &'a SecretString,
}

/// One page of a list, with the next page URL from `Link`.
pub struct Page<T> {
    pub items: T,
    pub next: Option<String>,
}

impl Caller<'_> {
    /// `GET url` with the token, read as JSON.
    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, HostError> {
        Ok(self.get_page(url).await?.items)
    }

    /// `GET url` with the token, read as JSON, with the next page's URL.
    pub async fn get_page<T: DeserializeOwned>(&self, url: &str) -> Result<Page<T>, HostError> {
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

        let next = reply
            .headers()
            .get(header::LINK)
            .and_then(|link| link.to_str().ok())
            .and_then(next_link)
            .map(str::to_owned);
        let items = reply.json::<T>().await.map_err(|e| {
            log::info!("reading open items from {host}: {e}");
            HostError::BadReply(host)
        })?;
        Ok(Page { items, next })
    }

    /// `PATCH url` with a JSON body, preserving the host's status for the caller.
    pub async fn patch<T: Serialize>(&self, url: &str, body: &T) -> Result<Response, HostError> {
        let _permit = self
            .calls
            .acquire()
            .await
            .map_err(|_| HostError::Internal(self.host.to_owned()))?;
        self.client
            .patch(url)
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("token {}", self.token.expose_secret()),
            )
            .json(body)
            .send()
            .await
            .map_err(|e| send_error(self.host, &e))
    }

    /// `POST url` with the token and a JSON body; the reply whatever its status.
    pub async fn post<T: Serialize>(&self, url: &str, body: &T) -> Result<Response, HostError> {
        let _permit = self
            .calls
            .acquire()
            .await
            .map_err(|_| HostError::Internal(self.host.to_owned()))?;
        self.client
            .post(url)
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("token {}", self.token.expose_secret()),
            )
            .json(body)
            .send()
            .await
            .map_err(|e| send_error(self.host, &e))
    }
}

/// The `rel="next"` URL of a `Link` header.
fn next_link(link: &str) -> Option<&str> {
    link.split(',').find_map(|part| {
        let (url, params) = part.trim().split_once(';')?;
        let next = params
            .split(';')
            .any(|param| param.trim().eq_ignore_ascii_case("rel=\"next\""));
        if !next {
            return None;
        }

        url.trim().strip_prefix('<')?.strip_suffix('>')
    })
}

/// Why a call that got no reply failed.
pub fn send_error(host: &str, error: &reqwest::Error) -> HostError {
    if error.is_timeout() {
        return HostError::Timeout(host.to_owned());
    }

    log::info!("asking {host}: {error}");
    HostError::Unreachable(host.to_owned())
}

/// Why a reply with the unsuccessful `status` failed.
pub fn status_error(host: String, status: StatusCode, headers: &HeaderMap) -> HostError {
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
