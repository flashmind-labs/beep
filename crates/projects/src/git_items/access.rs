//! Whether the user can write to a repository, as its host says. Home lists every open item of a
//! repository they can write to, and only their own in one they cannot, such as the upstream of
//! a fork.

pub use super::host::{Repo, RepoUrlError};
use super::{Caller, board::Login, github, host::HostError};
use crate::imports::hosts::Hosts;
use beep_proto::Service;
use futures::future::join_all;
use secrecy::ExposeSecret;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long an answer from the host is kept.
const KEEP: Duration = Duration::from_secs(3600);

#[derive(Debug, Deserialize)]
struct RepoReply {
    #[serde(default)]
    permissions: Option<Permissions>,
}

/// What GitHub and Forgejo both say the token may do on a repository.
#[derive(Debug, Default, Deserialize)]
struct Permissions {
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    push: bool,
}

impl RepoReply {
    fn writable(&self) -> bool {
        self.permissions
            .as_ref()
            .is_some_and(|can| can.push || can.admin)
    }
}

struct Answer {
    /// Which tokens it was asked with, see [`fingerprint`].
    tokens: u64,
    at: Instant,
    writable: bool,
}

static KNOWN: Mutex<Option<HashMap<Repo, Answer>>> = Mutex::new(None);

/// A one-way hash of the set of tokens, so an answer is asked again when they change. Kept in
/// memory only; the tokens themselves are never stored.
fn fingerprint(logins: &[Login<'_>]) -> u64 {
    let mut secrets: Vec<&str> = logins
        .iter()
        .map(|login| login.secret.expose_secret())
        .collect();
    secrets.sort_unstable();
    let mut hasher = DefaultHasher::new();
    secrets.hash(&mut hasher);
    hasher.finish()
}

/// The kept answer for `repo` given `tokens`, while it is fresh.
fn known(repo: &Repo, tokens: u64) -> Option<bool> {
    let known = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    let answer = known.as_ref()?.get(repo)?;
    (answer.tokens == tokens && answer.at.elapsed() < KEEP).then_some(answer.writable)
}

/// Keeps what the host said about `repo` to `tokens`.
fn remember(repo: Repo, tokens: u64, writable: bool) {
    KNOWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(
            repo,
            Answer {
                tokens,
                at: Instant::now(),
                writable,
            },
        );
}

/// The API address of `repo` on the host `login` is for.
fn repo_url(login: &Login<'_>, repo: &Repo) -> Option<String> {
    match login.service {
        Service::GitHub if repo.host == github::HOST => {
            Some(format!("https://api.github.com/repos/{}", repo.name))
        }
        Service::Forgejo { url } if url.host() == repo.host => Some(format!(
            "{}/api/v1/repos/{}",
            url.as_ref().trim_end_matches('/'),
            repo.name
        )),
        _ => None,
    }
}

/// Whether the user's tokens say they may push to `repo`; `None` when none could say.
async fn check(hosts: &dyn Hosts, logins: &[Login<'_>], repo: &Repo) -> Option<bool> {
    let tokens = fingerprint(logins);

    if let Some(writable) = known(repo, tokens) {
        return Some(writable);
    }

    let client = hosts.client().ok()?;
    let mut answered = false;

    for login in logins {
        let Some(url) = repo_url(login, repo) else {
            continue;
        };

        let caller = Caller {
            client: &client,
            calls: hosts.calls(),
            host: &repo.host,
            token: login.secret,
        };

        match caller.get::<RepoReply>(&url).await {
            Ok(reply) if reply.writable() => {
                remember(repo.clone(), tokens, true);
                return Some(true);
            }
            Ok(_) => answered = true,
            // A 404 is a private repository this token cannot see: not writable.
            Err(HostError::Status { status: 404, .. }) => answered = true,
            Err(e) => log::info!("write access to {} on {}: {e}", repo.name, repo.host),
        }
    }

    if answered {
        remember(repo.clone(), tokens, false);
    }

    answered.then_some(false)
}

/// What the tokens say about each of `repos`. A repository none could say anything about is left
/// out, for a machine to answer.
pub async fn token_answers(
    hosts: &dyn Hosts,
    logins: &[Login<'_>],
    repos: &HashSet<Repo>,
) -> HashMap<Repo, bool> {
    join_all(repos.iter().map(|repo| async move {
        let answer = check(hosts, logins, repo).await;
        answer.map(|writable| (repo.clone(), writable))
    }))
    .await
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
#[path = "../tests/git_items_access.rs"]
mod tests;
