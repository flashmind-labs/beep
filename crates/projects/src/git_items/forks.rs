//! The user's fork of a repository and their open pull request branches, read from GitHub or Forgejo.

use super::host::{Caller, HostError, Page};
use crate::model::{RepoName, branch_name};
use reqwest::Url;
use serde::{Deserialize, de::DeserializeOwned};
use std::str::FromStr;

/// The most forks, or open pull requests, one page reads.
const PAGE: &str = "50";
/// The most pages of one list read.
const MAX_PAGES: usize = 20;

/// Which API the calls go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    GitHub,
    Forgejo,
}

impl Api {
    /// The query that reads one page of a list.
    pub(super) fn page(self) -> String {
        match self {
            Api::GitHub => format!("per_page={PAGE}"),
            Api::Forgejo => format!("limit={PAGE}"),
        }
    }
}

/// The API base (such as `https://api.github.com`) and the repository there.
pub struct Lookup<'a> {
    pub api: Api,
    pub base: &'a str,
    pub upstream: &'a RepoName,
    /// The token's account, when it is known already; asked for otherwise.
    pub login: Option<&'a str>,
}

/// The user's fork, as the import clones it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fork {
    /// `login/repo`.
    pub name: RepoName,
    /// The upstream's default branch, which the local one tracks.
    pub default_branch: String,
    /// The heads of the user's open pull requests from the fork.
    pub branches: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Account {
    pub(super) login: String,
}

#[derive(Debug, Deserialize)]
struct Repo {
    full_name: String,
    #[serde(default)]
    default_branch: String,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    parent: Option<Named>,
    #[serde(default)]
    owner: Option<Account>,
}

#[derive(Debug, Deserialize)]
struct Named {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct Pull {
    #[serde(default)]
    user: Option<Account>,
    head: Head,
}

#[derive(Debug, Deserialize)]
struct Head {
    #[serde(rename = "ref")]
    branch: String,
    /// `None` once the fork the branch was on is deleted.
    #[serde(default)]
    repo: Option<Named>,
}

/// The user's fork of `lookup.upstream`, `None` when they have none or own the upstream.
pub async fn find(caller: &Caller<'_>, lookup: &Lookup<'_>) -> Result<Option<Fork>, HostError> {
    let base = lookup.base.trim_end_matches('/');
    let login = match lookup.login {
        Some(login) => login.to_owned(),
        None => caller.get::<Account>(&format!("{base}/user")).await?.login,
    };

    if lookup.upstream.owner().eq_ignore_ascii_case(&login) {
        return Ok(None);
    }

    let upstream = lookup.upstream.to_string();
    let parent: Repo = caller.get(&format!("{base}/repos/{upstream}")).await?;
    let Some(name) = fork_of(caller, lookup, base, &login, &parent.full_name).await? else {
        return Ok(None);
    };
    let pulls: Vec<Pull> = all_pages(
        caller,
        format!(
            "{base}/repos/{upstream}/pulls?state=open&{}",
            lookup.api.page()
        ),
        |_| false,
    )
    .await?;
    Ok(Some(Fork {
        branches: branches_from(&pulls, &login, &name),
        name,
        default_branch: parent.default_branch,
    }))
}

/// The fork of `upstream` owned by `login`, by name first, else from the upstream's fork list.
async fn fork_of(
    caller: &Caller<'_>,
    lookup: &Lookup<'_>,
    base: &str,
    login: &str,
    upstream: &str,
) -> Result<Option<RepoName>, HostError> {
    let same_name = format!("{base}/repos/{login}/{}", lookup.upstream.repo());

    match caller.get::<Repo>(&same_name).await {
        Ok(repo)
            if repo.fork
                && repo
                    .parent
                    .as_ref()
                    .is_some_and(|parent| parent.full_name.eq_ignore_ascii_case(upstream)) =>
        {
            return Ok(RepoName::from_str(&repo.full_name).ok());
        }
        Ok(_) | Err(HostError::Status { status: 404, .. }) => {}
        Err(e) => return Err(e),
    }

    let owned = |repo: &Repo| {
        repo.owner
            .as_ref()
            .is_some_and(|owner| owner.login.eq_ignore_ascii_case(login))
    };
    let forks: Vec<Repo> = all_pages(
        caller,
        format!("{base}/repos/{upstream}/forks?{}", lookup.api.page()),
        |page| page.iter().any(owned),
    )
    .await?;
    Ok(forks
        .iter()
        .find(|repo| owned(repo))
        .and_then(|repo| RepoName::from_str(&repo.full_name).ok()))
}

/// Every item of the list at `first`, following `Link` up to `MAX_PAGES` and only on the same host.
pub(super) async fn all_pages<T: DeserializeOwned>(
    caller: &Caller<'_>,
    first: String,
    found: impl Fn(&[T]) -> bool,
) -> Result<Vec<T>, HostError> {
    let origin = Url::parse(&first).ok().map(|url| url.origin());
    let mut items = Vec::new();
    let mut url = Some(first);

    for _ in 0..MAX_PAGES {
        let Some(at) = url.take() else {
            break;
        };
        let page: Page<Vec<T>> = caller.get_page(&at).await?;
        let done = found(&page.items);
        items.extend(page.items);

        if done {
            break;
        }
        url = page.next.filter(|next| {
            let next = Url::parse(next).ok().map(|url| url.origin());
            next.is_some() && next == origin
        });
    }

    Ok(items)
}

/// Head branches of `login`'s pull requests from `fork`, once each, skipping option-like names.
fn branches_from(pulls: &[Pull], login: &str, fork: &RepoName) -> Vec<String> {
    let fork = fork.to_string();
    let mut branches: Vec<String> = Vec::new();

    for pull in pulls {
        let mine = pull
            .user
            .as_ref()
            .is_some_and(|user| user.login.eq_ignore_ascii_case(login));
        let from_fork = pull
            .head
            .repo
            .as_ref()
            .is_some_and(|repo| repo.full_name.eq_ignore_ascii_case(&fork));
        let Some(branch) = branch_name(&pull.head.branch) else {
            continue;
        };
        if mine && from_fork && !branches.iter().any(|known| known == branch) {
            branches.push(branch.to_owned());
        }
    }

    branches
}

#[cfg(test)]
#[path = "../tests/git_items_forks.rs"]
mod tests;
