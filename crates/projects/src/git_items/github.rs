//! Open pull requests and issues on GitHub, from its issue search.

use super::host::{Caller, Found, HostError, Label, label_names};
use beep_proto::{GitItem, GitItemKind, GitReview};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use serde::Deserialize;
use std::{collections::HashMap, time::SystemTime};
use strum::AsRefStr;
use url::Url;

pub const HOST: &str = "github.com";
const SEARCH: &str = "https://api.github.com/search/issues";
/// Items each search reads; the overview needs no more.
const PER_PAGE: &str = "100";

/// One search and what its hits say about an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr)]
enum Query {
    /// PRs and issues the user opened.
    #[strum(serialize = "is:open is:pr author:@me archived:false")]
    AuthoredPulls,
    #[strum(serialize = "is:open is:issue author:@me archived:false")]
    AuthoredIssues,
    /// PRs and issues assigned to the user.
    #[strum(serialize = "is:open assignee:@me archived:false")]
    Assigned,
    /// PRs asking for the user's review.
    #[strum(serialize = "is:open is:pr review-requested:@me archived:false")]
    ReviewRequested,
    /// The user's PRs that are approved.
    #[strum(serialize = "is:open is:pr author:@me review:approved archived:false")]
    Approved,
    /// The user's PRs with changes requested.
    #[strum(serialize = "is:open is:pr author:@me review:changes_requested archived:false")]
    ChangesRequested,
    /// PRs the user is in whose checks fail.
    #[strum(serialize = "is:open is:pr involves:@me status:failure archived:false")]
    Failing,
}

impl Query {
    const ALL: [Query; 7] = [
        Query::AuthoredPulls,
        Query::AuthoredIssues,
        Query::Assigned,
        Query::ReviewRequested,
        Query::Approved,
        Query::ChangesRequested,
        Query::Failing,
    ];

    /// Whether its hits are items of their own rather than marks on others.
    fn lists(self) -> bool {
        matches!(
            self,
            Query::AuthoredPulls | Query::AuthoredIssues | Query::Assigned | Query::ReviewRequested
        )
    }
}

#[derive(Debug, Deserialize)]
struct SearchReply {
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    html_url: String,
    number: u64,
    title: String,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    comments: u32,
    #[serde(default)]
    draft: Option<bool>,
    /// Present on pull requests.
    #[serde(default)]
    pull_request: Option<serde::de::IgnoredAny>,
    /// `https://api.github.com/repos/<org>/<repo>`.
    repository_url: String,
    #[serde(default)]
    labels: Vec<Label>,
}

/// The user's open items on GitHub with `caller`'s token.
pub async fn items(caller: &Caller<'_>) -> Result<Vec<Found>, HostError> {
    let asked = join_all(Query::ALL.map(|query| async move {
        let url = Url::parse_with_params(
            SEARCH,
            &[
                ("q", query.as_ref()),
                ("per_page", PER_PAGE),
                ("sort", "updated"),
            ],
        )
        .map_err(|_| HostError::Internal(HOST.to_owned()))?;
        let reply: SearchReply = caller.get(url.as_str()).await?;
        Ok::<_, HostError>((query, reply.items))
    }))
    .await;
    let mut found = Vec::new();

    for (query, result) in Query::ALL.iter().zip(asked) {
        match result {
            Ok(hit) => found.push(hit),
            Err(e) if query.lists() => return Err(e),
            // The marking searches only add chips: without one, the items
            // still list, so its failure is logged rather than shown.
            Err(e) => log::info!("GitHub search {query:?} failed: {e}"),
        }
    }

    Ok(merge(found))
}

/// The items the listing searches found, once each, marked by the others.
fn merge(found: Vec<(Query, Vec<SearchItem>)>) -> Vec<Found> {
    let mut items: Vec<Found> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();

    for (query, hits) in found.iter().filter(|(query, _)| query.lists()) {
        for hit in hits {
            let Some(repo) = repo_of(&hit.repository_url) else {
                continue;
            };
            let i = *at.entry(hit.html_url.clone()).or_insert_with(|| {
                items.push(item(hit, repo));
                items.len() - 1
            });
            let item = &mut items[i].item;

            match query {
                Query::AuthoredPulls | Query::AuthoredIssues => item.authored = true,
                Query::Assigned => item.assigned = true,
                Query::ReviewRequested => item.review_requested = true,
                _ => {}
            }
        }
    }

    for (query, hits) in found.iter().filter(|(query, _)| !query.lists()) {
        for hit in hits {
            let Some(&i) = at.get(&hit.html_url) else {
                continue;
            };
            let item = &mut items[i].item;

            match query {
                Query::Approved => item.review = GitReview::Approved {},
                Query::ChangesRequested => item.review = GitReview::ChangesRequested {},
                Query::Failing => item.checks_failing = true,
                _ => {}
            }
        }
    }

    items
}

fn item(hit: &SearchItem, repo: String) -> Found {
    let pull = hit.pull_request.is_some();
    let item = GitItem {
        host: HOST.to_owned(),
        repo,
        number: hit.number,
        kind: if pull {
            GitItemKind::PullRequest {}
        } else {
            GitItemKind::Issue {}
        },
        title: hit.title.clone(),
        url: hit.html_url.clone(),
        updated_at: SystemTime::from(hit.updated_at),
        authored: false,
        assigned: false,
        review_requested: false,
        draft: pull && hit.draft.unwrap_or(false),
        review: GitReview::None {},
        checks_failing: false,
        comments: hit.comments,
    };
    Found {
        item,
        labels: label_names(&hit.labels),
        opened_at: hit.created_at.map(SystemTime::from),
    }
}

/// `org/repo` from an API repository URL.
fn repo_of(api_url: &str) -> Option<String> {
    let rest = api_url.strip_prefix("https://api.github.com/repos/")?;
    let (org, repo) = rest.trim_end_matches('/').split_once('/')?;
    (!org.is_empty() && !repo.is_empty() && !repo.contains('/')).then(|| format!("{org}/{repo}"))
}

#[cfg(test)]
#[path = "../tests/git_items_github.rs"]
mod tests;
