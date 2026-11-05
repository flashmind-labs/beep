//! Whether the user can write to a repository, as its host says. Home lists every open item of a
//! repository they can write to, and only their own in one they cannot, such as the upstream of
//! a fork.

use super::{Caller, board::Login, github, host::HostError};
use crate::{credentials::Slot, imports::hosts::Hosts};
use futures::future::join_all;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long an answer from the host is kept.
const KEEP: Duration = Duration::from_secs(3600);
/// How long a failed check is kept, so a host that is down is asked again soon.
const KEEP_FAILED: Duration = Duration::from_secs(60);

/// A repository by host and `org/repo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repo {
    pub host: String,
    pub name: String,
}

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
    at: Instant,
    ttl: Duration,
    writable: bool,
}

static KNOWN: Mutex<Option<HashMap<Repo, Answer>>> = Mutex::new(None);

fn known(repo: &Repo) -> Option<bool> {
    let known = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    let answer = known.as_ref()?.get(repo)?;
    (answer.at.elapsed() < answer.ttl).then_some(answer.writable)
}

fn remember(repo: Repo, writable: bool, ttl: Duration) {
    KNOWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(
            repo,
            Answer {
                at: Instant::now(),
                ttl,
                writable,
            },
        );
}

/// The API address of `repo` on the host `login` is for.
fn repo_url(login: &Login<'_>, repo: &Repo) -> Option<String> {
    match login.slot {
        Slot::GitHub if repo.host == github::HOST => {
            Some(format!("https://api.github.com/repos/{}", repo.name))
        }
        Slot::Forgejo(url) if url.host() == repo.host => Some(format!(
            "{}/api/v1/repos/{}",
            url.as_str().trim_end_matches('/'),
            repo.name
        )),
        _ => None,
    }
}

/// Whether any of the user's tokens may push to `repo`. A repository no token could be asked
/// about counts as not writable, so an upstream's list never floods Home.
async fn check(hosts: &dyn Hosts, logins: &[Login<'_>], repo: &Repo) -> bool {
    if let Some(writable) = known(repo) {
        return writable;
    }
    let Ok(client) = hosts.client() else {
        return false;
    };
    let mut failed = false;
    for login in logins {
        let Some(url) = repo_url(login, repo) else {
            continue;
        };
        if let Slot::Forgejo(base) = login.slot
            && !hosts.is_public(base).await
        {
            failed = true;
            continue;
        }
        let caller = Caller {
            client: &client,
            calls: hosts.calls(),
            host: &repo.host,
            token: login.secret,
        };
        match caller.get::<RepoReply>(&url).await {
            Ok(reply) if reply.writable() => {
                remember(repo.clone(), true, KEEP);
                return true;
            }
            Ok(_) => {}
            // A 404 is a private repository this token cannot see: not writable.
            Err(HostError::Status { status: 404, .. }) => {}
            Err(e) => {
                log::info!("write access to {} on {}: {e}", repo.name, repo.host);
                failed = true;
            }
        }
    }
    remember(repo.clone(), false, if failed { KEEP_FAILED } else { KEEP });
    false
}

/// The repositories of `repos` the user can write to.
pub async fn writable(
    hosts: &dyn Hosts,
    logins: &[Login<'_>],
    repos: HashSet<Repo>,
) -> HashSet<Repo> {
    join_all(repos.into_iter().map(|repo| async move {
        let can = check(hosts, logins, &repo).await;
        can.then_some(repo)
    }))
    .await
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(json: &str) -> RepoReply {
        serde_json::from_str(json).expect("a repository reply")
    }

    #[test]
    fn push_or_admin_is_write_access() {
        assert!(reply(r#"{"permissions":{"admin":false,"push":true,"pull":true}}"#).writable());
        assert!(reply(r#"{"permissions":{"admin":true,"push":false,"pull":true}}"#).writable());
    }

    #[test]
    fn read_only_or_missing_permissions_are_not_write_access() {
        assert!(!reply(r#"{"permissions":{"admin":false,"push":false,"pull":true}}"#).writable());
        assert!(!reply(r#"{"full_name":"acme/widget"}"#).writable());
    }
}
