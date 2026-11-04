//! The user's fork of an imported repository, so the import clones the fork with their PR branches.

use super::credential_of;
use crate::{
    account::credentials::{host_client, is_public},
    app::App,
};
use beep_projects::credentials::{Opened, Slot};
use beep_projects::{
    git_items::{Caller, forks::Api},
    imports::{
        fork::{UsedFork, find as find_fork},
        repo::RepoRef,
    },
};

const GITHUB_API: &str = "https://api.github.com";

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
    find_fork(&caller, api, &base, login, repo).await
}
