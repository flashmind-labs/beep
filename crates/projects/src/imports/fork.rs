//! The user's fork of an imported repository, so the import clones the fork with their PR branches.

use super::repo::RepoRef;
use crate::{
    git_items::{
        Caller,
        forks::{self, Api, Fork, Lookup},
    },
    model::RepoName,
};

/// The user's fork of an imported repository.
#[derive(Debug, Clone)]
pub struct UsedFork {
    /// The fork, on the repository's host.
    pub repo: RepoRef,
    /// What the host said about it.
    pub found: Fork,
}

/// The user's fork of `repo` on the host `caller` reaches, at `base` through `api`; `None` when
/// absent or unreadable, which is logged and clones the repository itself. The caller maps the
/// user's credential to the token and the host's API.
pub async fn find(
    caller: &Caller<'_>,
    api: Api,
    base: &str,
    login: Option<&str>,
    repo: &RepoRef,
) -> Option<UsedFork> {
    let upstream = RepoName::parse(&repo.project()).ok()?;
    let lookup = Lookup {
        api,
        base,
        upstream: &upstream,
        login,
    };
    let found = match forks::find(caller, &lookup).await {
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
