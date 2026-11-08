//! Making a repository on GitHub, Forgejo or Gitea with the user's token, and who it can belong to.

use super::{
    forks::{Account, Api, all_pages},
    host::{Caller, HostError, status_error},
};
use crate::model::RepoName;
use futures::future::join_all;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// A host's API, and the token's account when it is known already.
#[derive(Debug, Clone, Copy)]
pub struct Forge<'a> {
    pub api: Api,
    pub base: &'a str,
    pub login: Option<&'a str>,
}

impl Forge<'_> {
    async fn login(&self, caller: &Caller<'_>) -> Result<String, HostError> {
        match self.login {
            Some(login) => Ok(login.to_owned()),
            None => {
                let me: Account = caller.get(&format!("{}/user", self.base)).await?;
                Ok(me.login)
            }
        }
    }
}

/// An organization: GitHub calls it `login`, Forgejo `name`.
#[derive(Debug, Deserialize)]
struct Org {
    #[serde(default)]
    login: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrgPermissions {
    can_create_repository: bool,
}

/// The token's account, then the organizations it can create in.
pub async fn owners(caller: &Caller<'_>, forge: &Forge<'_>) -> Result<Vec<String>, HostError> {
    let login = forge.login(caller).await?;
    let first = format!("{}/user/orgs?{}", forge.base, forge.api.page());
    let orgs: Vec<Org> = match all_pages(caller, first, |_| false).await {
        Ok(orgs) => orgs,
        // A token that cannot read organizations can still create under its own account.
        Err(e) => {
            log::info!("listing organizations: {e}");
            Vec::new()
        }
    };
    let names = orgs.into_iter().filter_map(|org| org.login.or(org.name));
    let kept = join_all(names.map(|org| {
        let login = &login;
        async move { can_create(caller, forge, login, &org).await.then_some(org) }
    }))
    .await;
    let mut all = vec![login];
    all.extend(kept.into_iter().flatten());
    Ok(all)
}

/// Whether `login` can create repositories in `org`. GitHub only tells an org's owners, so there the
/// answer waits for the create call.
async fn can_create(caller: &Caller<'_>, forge: &Forge<'_>, login: &str, org: &str) -> bool {
    if forge.api == Api::GitHub {
        return true;
    }
    let url = format!("{}/users/{login}/orgs/{org}/permissions", forge.base);
    match caller.get::<OrgPermissions>(&url).await {
        Ok(permissions) => permissions.can_create_repository,
        Err(e) => {
            log::info!("reading permissions in {org}: {e}");
            false
        }
    }
}

/// The repository to make.
#[derive(Debug, Clone, Copy)]
pub struct NewRepo<'a> {
    pub owner: &'a str,
    pub name: &'a str,
    pub private: bool,
}

#[derive(Debug, Serialize)]
struct Body<'a> {
    name: &'a str,
    private: bool,
    /// A first commit, so the clones have a branch to work from.
    auto_init: bool,
}

#[derive(Debug, Deserialize)]
struct Made {
    full_name: String,
}

/// Why the host made no repository.
#[derive(Debug, PartialEq, Eq)]
pub enum CreateError {
    NameTaken,
    /// The token lacks the scope, or the account may not create in that owner.
    Denied,
    Host(HostError),
}

/// Creates the repository and gives its name as the host wrote it.
pub async fn create(
    caller: &Caller<'_>,
    forge: &Forge<'_>,
    new: &NewRepo<'_>,
) -> Result<RepoName, CreateError> {
    let login = forge.login(caller).await.map_err(CreateError::Host)?;
    let url = if new.owner.eq_ignore_ascii_case(&login) {
        format!("{}/user/repos", forge.base)
    } else {
        format!("{}/orgs/{}/repos", forge.base, new.owner)
    };
    let body = Body {
        name: new.name,
        private: new.private,
        auto_init: true,
    };
    let reply = caller.post(&url, &body).await.map_err(CreateError::Host)?;
    let host = caller.host.to_owned();
    let status = reply.status();
    if status.is_success() {
        let bad = || CreateError::Host(HostError::BadReply(host.clone()));
        let made: Made = reply.json().await.map_err(|_| bad())?;
        return RepoName::from_str(&made.full_name).map_err(|_| bad());
    }
    let headers = reply.headers().clone();
    // Read only for GitHub's sentence; a reply that cannot be read counts as any other refusal.
    let said = reply.text().await.unwrap_or_default();
    Err(match status_error(host, status, &headers) {
        limited @ HostError::RateLimited(_) => CreateError::Host(limited),
        _ if status == StatusCode::CONFLICT => CreateError::NameTaken,
        _ if status == StatusCode::UNPROCESSABLE_ENTITY && said.contains("already exists") => {
            CreateError::NameTaken
        }
        _ if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND => {
            CreateError::Denied
        }
        other => CreateError::Host(other),
    })
}

#[cfg(test)]
#[path = "../tests/git_items_new_repo.rs"]
mod tests;
