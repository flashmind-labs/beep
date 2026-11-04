//! The user's fork of an imported repository, so the import clones the fork with their PR branches.

use super::credential_of;
use crate::{
    account::credentials::{Opened, Slot, host_client, is_public},
    app::App,
};
use beep_projects::git_items::forks;
use beep_projects::imports::repo::RepoRef;
use beep_projects::{
    git_items::{
        Caller,
        forks::{Api, Lookup},
    },
    model::RepoName,
};

const GITHUB_API: &str = "https://api.github.com";

pub(crate) use beep_projects::imports::clone::UsedFork;

/// Where to reach a repository's host, and with which token.
pub(super) struct Target<'a> {
    pub(super) opened: &'a Opened,
    pub(super) api: Api,
    /// The API's base URL.
    pub(super) base: String,
    /// The token's account, when the saved credential says.
    pub(super) login: Option<&'a str>,
}

/// The API `opened` is a token for; `None` for an unreachable local Forgejo or another kind of token.
pub(super) async fn target_of(opened: &Opened) -> Option<Target<'_>> {
    Some(match &opened.slot {
        Slot::GitHub => Target {
            opened,
            api: Api::GitHub,
            base: GITHUB_API.to_owned(),
            login: opened.github.as_ref().map(|github| github.login.as_str()),
        },
        Slot::Forgejo(url) => {
            if !is_public(url).await {
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
pub(crate) async fn find(app: &App, credentials: &[Opened], repo: &RepoRef) -> Option<UsedFork> {
    let Target {
        opened,
        api,
        base,
        login,
    } = target_of(credential_of(repo, credentials)?).await?;
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
