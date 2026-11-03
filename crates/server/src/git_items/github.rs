//! Open pull requests and issues on GitHub, from its issue search.

use super::host::{Caller, HostError};
use crate::protocol::{GitItem, GitItemKind, GitReview};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use serde::Deserialize;
use std::{collections::HashMap, time::SystemTime};
use url::Url;

pub const HOST: &str = "github.com";
const SEARCH: &str = "https://api.github.com/search/issues";
/// Items each search reads; the overview needs no more.
const PER_PAGE: &str = "50";

/// One search and what its hits say about an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Query {
    /// PRs and issues the user opened.
    Authored,
    /// Issues assigned to the user.
    Assigned,
    /// PRs asking for the user's review.
    ReviewRequested,
    /// The user's PRs that are approved.
    Approved,
    /// The user's PRs with changes requested.
    ChangesRequested,
    /// PRs the user is in whose checks fail.
    Failing,
}

impl Query {
    const ALL: [Query; 6] = [
        Query::Authored,
        Query::Assigned,
        Query::ReviewRequested,
        Query::Approved,
        Query::ChangesRequested,
        Query::Failing,
    ];

    fn text(self) -> &'static str {
        match self {
            Query::Authored => "is:open author:@me archived:false",
            Query::Assigned => "is:open is:issue assignee:@me archived:false",
            Query::ReviewRequested => "is:open is:pr review-requested:@me archived:false",
            Query::Approved => "is:open is:pr author:@me review:approved archived:false",
            Query::ChangesRequested => {
                "is:open is:pr author:@me review:changes_requested archived:false"
            }
            Query::Failing => "is:open is:pr involves:@me status:failure archived:false",
        }
    }

    /// Whether its hits are items of their own rather than marks on others.
    fn lists(self) -> bool {
        matches!(
            self,
            Query::Authored | Query::Assigned | Query::ReviewRequested
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
    comments: u32,
    #[serde(default)]
    draft: Option<bool>,
    /// Present on pull requests.
    #[serde(default)]
    pull_request: Option<serde::de::IgnoredAny>,
    /// `https://api.github.com/repos/<org>/<repo>`.
    repository_url: String,
}

/// The user's open items on GitHub with `caller`'s token.
pub async fn items(caller: &Caller<'_>) -> Result<Vec<GitItem>, HostError> {
    let asked = join_all(Query::ALL.map(|query| async move {
        let url = Url::parse_with_params(
            SEARCH,
            &[
                ("q", query.text()),
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
fn merge(found: Vec<(Query, Vec<SearchItem>)>) -> Vec<GitItem> {
    let mut items: Vec<GitItem> = Vec::new();
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
            let item = &mut items[i];
            match query {
                Query::Authored => item.authored = true,
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
            let item = &mut items[i];
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

fn item(hit: &SearchItem, repo: String) -> GitItem {
    let pull = hit.pull_request.is_some();
    GitItem {
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
    }
}

/// `org/repo` from an API repository URL.
fn repo_of(api_url: &str) -> Option<String> {
    let rest = api_url.strip_prefix("https://api.github.com/repos/")?;
    let (org, repo) = rest.trim_end_matches('/').split_once('/')?;
    (!org.is_empty() && !repo.is_empty() && !repo.contains('/')).then(|| format!("{org}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(json: &str) -> Vec<SearchItem> {
        serde_json::from_str::<SearchReply>(json)
            .expect("the fixture is a search reply")
            .items
    }

    const AUTHORED: &str = r#"{"total_count": 2, "incomplete_results": false, "items": [
      {"html_url": "https://github.com/acme/widget/pull/142", "number": 142,
       "title": "Retry uploads when the token expires", "state": "open", "draft": false,
       "comments": 3, "updated_at": "2026-10-30T09:15:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget",
       "user": {"login": "some-user"},
       "pull_request": {"url": "https://api.github.com/repos/acme/widget/pulls/142"}},
      {"html_url": "https://github.com/acme/widget/issues/131", "number": 131,
       "title": "Crash when the camera permission is denied", "state": "open",
       "comments": 0, "updated_at": "2026-10-29T08:00:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget",
       "user": {"login": "some-user"}}
    ]}"#;

    const ASSIGNED: &str = r#"{"total_count": 2, "items": [
      {"html_url": "https://github.com/acme/widget/issues/131", "number": 131,
       "title": "Crash when the camera permission is denied", "comments": 0,
       "updated_at": "2026-10-29T08:00:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget"},
      {"html_url": "https://github.com/tools-org/uploader/issues/48", "number": 48,
       "title": "Resume after the laptop sleeps", "comments": 1,
       "updated_at": "2026-10-20T08:00:00Z",
       "repository_url": "https://api.github.com/repos/tools-org/uploader"}
    ]}"#;

    const REVIEW: &str = r#"{"total_count": 1, "items": [
      {"html_url": "https://github.com/acme/widget/pull/144", "number": 144,
       "title": "Add a dark theme", "draft": true, "comments": 0,
       "updated_at": "2026-10-31T10:00:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget",
       "pull_request": {}}
    ]}"#;

    const FAILING: &str = r#"{"total_count": 2, "items": [
      {"html_url": "https://github.com/acme/widget/pull/142", "number": 142, "title": "x",
       "updated_at": "2026-10-30T09:15:00Z",
       "repository_url": "https://api.github.com/repos/acme/widget", "pull_request": {}},
      {"html_url": "https://github.com/other/thing/pull/9", "number": 9, "title": "y",
       "updated_at": "2026-10-30T09:15:00Z",
       "repository_url": "https://api.github.com/repos/other/thing", "pull_request": {}}
    ]}"#;

    #[test]
    fn search_items_parse_into_prs_and_issues() {
        let items = merge(vec![(Query::Authored, reply(AUTHORED))]);
        assert_eq!(items.len(), 2);
        let pr = &items[0];
        assert_eq!(pr.kind, GitItemKind::PullRequest {});
        assert_eq!(pr.repo, "acme/widget");
        assert_eq!(pr.number, 142);
        assert_eq!(pr.host, "github.com");
        assert_eq!(pr.url, "https://github.com/acme/widget/pull/142");
        assert_eq!(pr.comments, 3);
        assert!(pr.authored && !pr.assigned && !pr.draft);
        assert_eq!(
            pr.updated_at,
            SystemTime::from(
                "2026-10-30T09:15:00Z"
                    .parse::<DateTime<Utc>>()
                    .expect("a valid time")
            )
        );
        assert_eq!(items[1].kind, GitItemKind::Issue {});
    }

    #[test]
    fn an_item_two_searches_find_is_listed_once_with_both_marks() {
        let items = merge(vec![
            (Query::Authored, reply(AUTHORED)),
            (Query::Assigned, reply(ASSIGNED)),
            (Query::ReviewRequested, reply(REVIEW)),
        ]);
        assert_eq!(items.len(), 4);
        let crash = items
            .iter()
            .find(|i| i.number == 131)
            .expect("the issue is listed");
        assert!(crash.authored && crash.assigned);
        let review = items
            .iter()
            .find(|i| i.number == 144)
            .expect("the PR is listed");
        assert!(review.review_requested && review.draft && !review.authored);
    }

    #[test]
    fn marking_searches_mark_but_never_add() {
        let items = merge(vec![
            (Query::Failing, reply(FAILING)),
            (Query::Authored, reply(AUTHORED)),
            (Query::ChangesRequested, reply(FAILING)),
        ]);
        assert_eq!(items.len(), 2);
        assert!(items[0].checks_failing);
        assert_eq!(items[0].review, GitReview::ChangesRequested {});
        assert!(!items[1].checks_failing);
        assert!(items.iter().all(|i| i.repo != "other/thing"));
    }

    #[test]
    fn repos_come_from_the_api_url() {
        assert_eq!(
            repo_of("https://api.github.com/repos/acme/widget").as_deref(),
            Some("acme/widget")
        );
        assert_eq!(repo_of("https://example.com/repos/acme/widget"), None);
        assert_eq!(repo_of("https://api.github.com/repos/acme"), None);
    }
}
