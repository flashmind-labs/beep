//! The user's fork of a repository they import: found with their token on
//! the repository's host, so the import clones the fork, where their pull
//! request branches are, rather than the repository itself.

use super::repo::{Host, RepoRef};
use crate::{
    app::App,
    credentials::{Opened, Slot, github_for, host_client},
    git_items::{
        Caller,
        forks::{self, Api, Fork, Lookup},
    },
    projects::model::RepoName,
};
use secrecy::ExposeSecret;

const GITHUB_API: &str = "https://api.github.com";

/// The user's fork of an imported repository.
#[derive(Debug, Clone)]
pub(crate) struct UsedFork {
    /// The fork, on the repository's host.
    pub(crate) repo: RepoRef,
    /// What the host said about it.
    pub(crate) found: Fork,
}

/// Where to ask about forks of a repository, and with which token.
struct Target<'a> {
    opened: &'a Opened,
    api: Api,
    /// The API's base URL.
    base: String,
    /// The token's account, when the saved credential says.
    login: Option<&'a str>,
}

/// The API and token for `repo`'s host; `None` without a token there, or
/// for a Forgejo server on the user's own network, which the server
/// cannot reach.
async fn target<'a>(credentials: &'a [Opened], repo: &RepoRef) -> Option<Target<'a>> {
    let target = match &repo.host {
        Host::GitHub => {
            let opened = github_for(credentials, repo.org())?;
            Target {
                opened,
                api: Api::GitHub,
                base: GITHUB_API.to_owned(),
                login: opened.github.as_ref().map(|github| github.login.as_str()),
            }
        }
        Host::Forgejo(url) => {
            let opened = credentials
                .iter()
                .find(|opened| matches!(&opened.slot, Slot::Forgejo(saved) if saved == url))?;
            if !url.is_public().await {
                return None;
            }
            Target {
                opened,
                api: Api::Forgejo,
                base: format!("{}/api/v1", url.as_str()),
                login: None,
            }
        }
    };
    (!target.opened.secret.expose_secret().is_empty()).then_some(target)
}

/// The user's fork of `repo`, read with their token for its host. `None`
/// when they have none, no token there, or the host could not be read: a
/// failure here is logged and the import clones the repository itself, as
/// it did before forks were looked for.
pub(crate) async fn find(app: &App, credentials: &[Opened], repo: &RepoRef) -> Option<UsedFork> {
    let Target {
        opened,
        api,
        base,
        login,
    } = target(credentials, repo).await?;
    let upstream = RepoName::parse(&repo.project()).ok()?;
    let client = match host_client() {
        Ok(client) => client,
        Err(e) => {
            log::warn!("no client to look for a fork: {e:#}");
            return None;
        }
    };
    let host = repo.host_name();
    let caller = Caller {
        client: &client,
        calls: app.git_items.calls(),
        host: &host,
        token: &opened.secret,
    };
    let lookup = Lookup {
        api,
        base: &base,
        upstream: &upstream,
        login,
    };
    let found = match forks::find(&caller, &lookup).await {
        Ok(found) => found?,
        Err(e) => {
            log::info!("looking for a fork of {}: {e}", repo.project());
            return None;
        }
    };
    match repo.sibling(&found.name) {
        Ok(fork) => Some(UsedFork { repo: fork, found }),
        Err(e) => {
            log::info!("the fork {} is not a repository name: {e}", found.name);
            None
        }
    }
}
