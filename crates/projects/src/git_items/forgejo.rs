//! Open pull requests and issues on a Forgejo or Gitea server.

use super::host::{Caller, Found, HostError, Label, label_names};
use beep_proto::{GitItem, GitItemKind, GitReview};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use serde::Deserialize;
use std::{collections::HashMap, time::SystemTime};
use url::Url;

/// Items each search reads.
const LIMIT: &str = "50";

/// One search and what its hits say about an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Query {
    AuthoredPulls,
    ReviewRequested,
    AuthoredIssues,
    AssignedIssues,
    AssignedPulls,
}

impl Query {
    const ALL: [Query; 5] = [
        Query::AuthoredPulls,
        Query::ReviewRequested,
        Query::AuthoredIssues,
        Query::AssignedIssues,
        Query::AssignedPulls,
    ];
}

impl From<Query> for [(&'static str, &'static str); 2] {
    /// The search's `type` and the filter it sets.
    fn from(query: Query) -> Self {
        match query {
            Query::AuthoredPulls => [("type", "pulls"), ("created", "true")],
            Query::ReviewRequested => [("type", "pulls"), ("review_requested", "true")],
            Query::AuthoredIssues => [("type", "issues"), ("created", "true")],
            Query::AssignedIssues => [("type", "issues"), ("assigned", "true")],
            Query::AssignedPulls => [("type", "pulls"), ("assigned", "true")],
        }
    }
}

#[derive(Debug, Deserialize)]
struct Issue {
    html_url: String,
    number: u64,
    title: String,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    comments: u32,
    /// Present on pull requests.
    #[serde(default)]
    pull_request: Option<PullMeta>,
    repository: Repository,
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Debug, Deserialize)]
struct PullMeta {
    /// Newer servers say; older ones mark a draft by its title.
    #[serde(default)]
    draft: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct Repository {
    full_name: String,
}

/// The user's open items on the Forgejo server at `base` with `caller`'s token. Whether the
/// server may be called from here (it is not on a private network) is the caller's check; this
/// does not make it.
pub async fn items(caller: &Caller<'_>, base: &str) -> Result<Vec<Found>, HostError> {
    let search = format!("{base}/api/v1/repos/issues/search");
    let asked = join_all(Query::ALL.map(|query| {
        let search = &search;
        async move {
            let mut params = vec![("state", "open"), ("limit", LIMIT)];
            params.extend(<[(&str, &str); 2]>::from(query));
            let url = Url::parse_with_params(search, &params)
                .map_err(|_| HostError::Internal(caller.host.to_owned()))?;
            let issues: Vec<Issue> = caller.get(url.as_str()).await?;
            Ok::<_, HostError>((query, issues))
        }
    }))
    .await;
    let found = asked.into_iter().collect::<Result<Vec<_>, _>>()?;
    Ok(merge(caller.host, found))
}

/// What the searches found, once each.
fn merge(host: &str, found: Vec<(Query, Vec<Issue>)>) -> Vec<Found> {
    let mut items: Vec<Found> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();

    for (query, issues) in &found {
        for issue in issues {
            let i = *at.entry(issue.html_url.clone()).or_insert_with(|| {
                items.push(item(host, issue));
                items.len() - 1
            });
            let item = &mut items[i].item;

            match query {
                Query::AuthoredPulls | Query::AuthoredIssues => item.authored = true,
                Query::ReviewRequested => item.review_requested = true,
                Query::AssignedIssues | Query::AssignedPulls => item.assigned = true,
            }
        }
    }

    items
}

fn item(host: &str, issue: &Issue) -> Found {
    let title = issue.title.trim_start();
    let wip = ["WIP:", "[WIP]", "Draft:", "[Draft]"].iter().any(|mark| {
        title
            .to_ascii_lowercase()
            .starts_with(&mark.to_ascii_lowercase())
    });
    let item = GitItem {
        host: host.to_owned(),
        repo: issue.repository.full_name.clone(),
        number: issue.number,
        kind: if issue.pull_request.is_some() {
            GitItemKind::PullRequest {}
        } else {
            GitItemKind::Issue {}
        },
        title: issue.title.clone(),
        url: issue.html_url.clone(),
        updated_at: SystemTime::from(issue.updated_at),
        authored: false,
        assigned: false,
        review_requested: false,
        draft: issue
            .pull_request
            .as_ref()
            .is_some_and(|pull| pull.draft.unwrap_or(wip)),
        review: GitReview::None {},
        checks_failing: false,
        comments: issue.comments,
    };
    Found {
        item,
        labels: label_names(&issue.labels),
        opened_at: issue.created_at.map(SystemTime::from),
    }
}

#[cfg(test)]
#[path = "../tests/git_items_forgejo.rs"]
mod tests;
