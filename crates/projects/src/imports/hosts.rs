//! Reaching the user's git hosts for a repository: which API and token, a fork lookup, the owners
//! a new repository can go under, and making one.

use super::{
    ImportError,
    fork::UsedFork,
    repo::{Host, RepoRef},
};
use crate::{
    credentials::{Opened, Slot},
    git_items::{
        Caller, HostError,
        forks::Api,
        new_repo::{self, CreateError, Forge, NewRepo},
    },
    model::ForgejoUrl,
};
use anyhow::Result;
use async_trait::async_trait;
use beep_proto::{GitHostKind, NewRepository, ProjectForge};
use futures::future::join_all;
use reqwest::Client;
use secrecy::ExposeSecret;
use std::sync::Arc;
use tokio::sync::Semaphore;

const GITHUB: &str = "github.com";
const GITHUB_API: &str = "https://api.github.com";

/// What reaching a git host needs from where the code runs: a client, the limit on calls in
/// flight, and whether a Forgejo server can be reached from here.
#[async_trait]
pub trait Hosts: Send + Sync {
    fn client(&self) -> Result<Client>;

    fn calls(&self) -> &Arc<Semaphore>;

    /// Whether every address the server's host resolves to is public, so the server may call it.
    async fn is_public(&self, url: &ForgejoUrl) -> bool;
}

/// The Forgejo servers among the user's credentials.
pub fn forgejo_of(credentials: &[Opened]) -> Vec<ForgejoUrl> {
    credentials
        .iter()
        .filter_map(|opened| match &opened.slot {
            Slot::Forgejo(url) => Some(url.clone()),
            _ => None,
        })
        .collect()
}

/// The user's non-empty token for `repo`'s host (on GitHub, the owner's account).
pub fn credential_of<'a>(repo: &RepoRef, credentials: &'a [Opened]) -> Option<&'a Opened> {
    match &repo.host {
        Host::GitHub => crate::credentials::github_for(credentials, repo.org()),
        Host::Forgejo(url) => credentials
            .iter()
            .find(|opened| matches!(&opened.slot, Slot::Forgejo(saved) if saved == url)),
    }
    .filter(|opened| !opened.secret.expose_secret().is_empty())
}

/// Where to reach a repository's host, and with which token.
pub struct Target<'a> {
    pub opened: &'a Opened,
    pub api: Api,
    /// The API's base URL.
    pub base: String,
    /// The token's account, when the saved credential says.
    pub login: Option<&'a str>,
}

/// The API `opened` is a token for; `None` for an unreachable local Forgejo or another kind of token.
pub async fn target_of<'a>(hosts: &dyn Hosts, opened: &'a Opened) -> Option<Target<'a>> {
    Some(match &opened.slot {
        Slot::GitHub => Target {
            opened,
            api: Api::GitHub,
            base: GITHUB_API.to_owned(),
            login: opened.github.as_ref().map(|github| github.login.as_str()),
        },
        Slot::Forgejo(url) => {
            if !hosts.is_public(url).await {
                return None;
            }
            Target {
                opened,
                api: Api::Forgejo,
                base: format!("{}/api/v1", url.as_str()),
                login: None,
            }
        }
        _ => return None,
    })
}

/// The user's fork of `repo`; `None` when absent or unreadable, which is logged and clones the repo.
pub async fn find_fork(
    hosts: &dyn Hosts,
    credentials: &[Opened],
    repo: &RepoRef,
) -> Option<UsedFork> {
    let Target {
        opened,
        api,
        base,
        login,
    } = target_of(hosts, credential_of(repo, credentials)?).await?;
    let client = match hosts.client() {
        Ok(client) => client,
        Err(e) => {
            log::warn!("no client to look for a fork: {e:#}");
            return None;
        }
    };
    let host = repo.host_name();
    let caller = Caller {
        client: &client,
        calls: hosts.calls(),
        host: &host,
        token: &opened.secret,
    };
    super::fork::find(&caller, api, &base, login, repo).await
}

/// Each git host the user has a token for, with who a repository there can belong to.
pub async fn project_forges(
    hosts: &dyn Hosts,
    credentials: &[Opened],
) -> Result<Vec<ProjectForge>, ImportError> {
    let client = hosts.client()?;
    let usable = |opened: &&Opened| !opened.secret.expose_secret().is_empty();
    let mut github: Vec<&Opened> = credentials
        .iter()
        .filter(|opened| opened.github.is_some())
        .filter(usable)
        .collect();
    github.sort_by_key(|opened| !opened.github.as_ref().is_some_and(|login| login.default));
    let github = async {
        if github.is_empty() {
            return None;
        }
        let kind = GitHostKind::GitHub {};
        Some(forge_row(hosts, &client, GITHUB.to_owned(), kind, &github).await)
    };
    let forgejo = credentials.iter().filter(usable).filter_map(|opened| {
        let Slot::Forgejo(url) = &opened.slot else {
            return None;
        };
        let client = &client;
        Some(async move {
            let kind = GitHostKind::Forgejo {};
            forge_row(hosts, client, url.host(), kind, &[opened]).await
        })
    });
    let (github, forgejo) = tokio::join!(github, join_all(forgejo));
    Ok(github.into_iter().chain(forgejo).collect())
}

/// One host's row: the owners every token of `accounts` gives, the error when none answered.
async fn forge_row(
    hosts: &dyn Hosts,
    client: &Client,
    host: String,
    kind: GitHostKind,
    accounts: &[&Opened],
) -> ProjectForge {
    let name = host.as_str();
    let asked = join_all(accounts.iter().map(|opened| async move {
        let target = target_of(hosts, opened)
            .await
            .ok_or_else(|| HostError::Private(name.to_owned()))?;
        let caller = Caller {
            client,
            calls: hosts.calls(),
            host: name,
            token: &opened.secret,
        };
        let forge = Forge {
            api: target.api,
            base: &target.base,
            login: target.login,
        };
        new_repo::owners(&caller, &forge).await
    }))
    .await;
    let mut owners: Vec<String> = Vec::new();
    let mut error = None;
    for answer in asked {
        match answer {
            Ok(found) => {
                for owner in found {
                    if !owners
                        .iter()
                        .any(|known| known.eq_ignore_ascii_case(&owner))
                    {
                        owners.push(owner);
                    }
                }
            }
            Err(e) => error = Some(e.to_string()),
        }
    }
    ProjectForge {
        host,
        kind,
        error: error.filter(|_| owners.is_empty()),
        owners,
    }
}

/// Creates the repository on its host with the owner's token.
pub async fn create_repository(
    hosts: &dyn Hosts,
    credentials: &[Opened],
    new: &NewRepository,
    name: &str,
) -> Result<RepoRef, ImportError> {
    let host = if new.host == GITHUB {
        Host::GitHub
    } else {
        let server = forgejo_of(credentials)
            .into_iter()
            .find(|server| server.host() == new.host);
        Host::Forgejo(server.ok_or_else(|| ImportError::UnknownHost(new.host.clone()))?)
    };
    let wanted = RepoRef::new(host, &new.owner, name)?;
    let host_name = wanted.host_name();
    let opened = credential_of(&wanted, credentials)
        .ok_or_else(|| ImportError::NoToken(host_name.clone()))?;
    let target = target_of(hosts, opened)
        .await
        .ok_or_else(|| ImportError::Forge(HostError::Private(host_name.clone())))?;
    let client = hosts.client()?;
    let caller = Caller {
        client: &client,
        calls: hosts.calls(),
        host: &host_name,
        token: &opened.secret,
    };
    let forge = Forge {
        api: target.api,
        base: &target.base,
        login: target.login,
    };
    let repository = NewRepo {
        owner: &new.owner,
        name,
        private: new.private,
    };
    let made = new_repo::create(&caller, &forge, &repository)
        .await
        .map_err(|e| match e {
            CreateError::NameTaken => ImportError::NameTaken {
                repo: wanted.project(),
                host: host_name.clone(),
            },
            CreateError::Denied => ImportError::CannotCreate {
                owner: new.owner.clone(),
                host: host_name.clone(),
            },
            CreateError::Host(e) => ImportError::Forge(e),
        })?;
    log::info!("repository created; project={made} host={host_name}");
    wanted.sibling(&made)
}
