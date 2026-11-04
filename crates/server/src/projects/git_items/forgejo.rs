//! Open pull requests and issues on a Forgejo or Gitea server.

use super::host::{Caller, Found, HostError, Label, label_names};
use crate::{
    account::credentials::ForgejoUrl,
    protocol::{GitItem, GitItemKind, GitReview},
};
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
}

impl Query {
    const ALL: [Query; 4] = [
        Query::AuthoredPulls,
        Query::ReviewRequested,
        Query::AuthoredIssues,
        Query::AssignedIssues,
    ];

    /// The search's `type` and the filter it sets.
    fn params(self) -> [(&'static str, &'static str); 2] {
        match self {
            Query::AuthoredPulls => [("type", "pulls"), ("created", "true")],
            Query::ReviewRequested => [("type", "pulls"), ("review_requested", "true")],
            Query::AuthoredIssues => [("type", "issues"), ("created", "true")],
            Query::AssignedIssues => [("type", "issues"), ("assigned", "true")],
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

/// The user's open items on the server at `url` with `caller`'s token.
pub async fn items(caller: &Caller<'_>, url: &ForgejoUrl) -> Result<Vec<Found>, HostError> {
    if !url.is_public().await {
        return Err(HostError::Private(caller.host.to_owned()));
    }
    let search = format!("{}/api/v1/repos/issues/search", url.as_str());
    let asked = join_all(Query::ALL.map(|query| {
        let search = &search;
        async move {
            let mut params = vec![("state", "open"), ("limit", LIMIT)];
            params.extend(query.params());
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
                Query::AssignedIssues => item.assigned = true,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues(json: &str) -> Vec<Issue> {
        serde_json::from_str(json).expect("the fixture is a list of issues")
    }

    const PULLS: &str = r#"[
      {"id": 901, "number": 57, "title": "WIP: Stream large files in chunks",
       "html_url": "https://git.example.com/tools-org/uploader/pulls/57",
       "state": "open", "comments": 2, "updated_at": "2026-10-28T12:00:00+01:00",
       "user": {"login": "some-user"},
       "labels": [{"id": 3, "name": "perf", "color": "ee0701"}],
       "pull_request": {"merged": false, "merged_at": null},
       "repository": {"id": 4, "name": "uploader", "owner": "tools-org",
                      "full_name": "tools-org/uploader"}}
    ]"#;

    const REVIEW: &str = r#"[
      {"number": 61, "title": "Drop the old v1 endpoints",
       "html_url": "https://git.example.com/tools-org/uploader/pulls/61",
       "comments": 0, "updated_at": "2026-10-31T08:00:00Z",
       "pull_request": {"merged": false, "draft": true},
       "repository": {"full_name": "tools-org/uploader"}}
    ]"#;

    const ISSUES: &str = r#"[
      {"number": 48, "title": "Resume after the laptop sleeps",
       "html_url": "https://git.example.com/tools-org/uploader/issues/48",
       "comments": 1, "updated_at": "2026-10-20T08:00:00Z", "pull_request": null,
       "repository": {"full_name": "tools-org/uploader"}}
    ]"#;

    #[test]
    fn issues_parse_with_their_repository() {
        let items = merge(
            "git.example.com",
            vec![
                (Query::AuthoredPulls, issues(PULLS)),
                (Query::ReviewRequested, issues(REVIEW)),
                (Query::AuthoredIssues, issues(ISSUES)),
                (Query::AssignedIssues, issues(ISSUES)),
            ],
        );
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].labels, ["perf"]);
        assert!(items[2].labels.is_empty());
        let pr = &items[0].item;
        assert_eq!(pr.host, "git.example.com");
        assert_eq!(pr.repo, "tools-org/uploader");
        assert_eq!(pr.kind, GitItemKind::PullRequest {});
        assert!(pr.authored && pr.draft, "a WIP title marks a draft");
        assert_eq!(pr.comments, 2);
        let review = &items[1].item;
        assert!(review.review_requested && review.draft && !review.authored);
        let issue = &items[2].item;
        assert_eq!(issue.kind, GitItemKind::Issue {});
        assert!(issue.authored && issue.assigned && !issue.draft);
    }
}
