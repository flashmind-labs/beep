//! One pull request or issue read in full from the project's GitHub or Forgejo
//! host, for the project page's detail pane.

use super::{fleet::Fleet, issues::IssueError, threads::clipped};
use crate::projects::stored::StoredProject;
use crate::{
    account::user::User,
    protocol::{GitItemKind, GitReview, RunState, ThreadDetail, ThreadPost, ThreadState},
    store::ProjectRow,
};
use anyhow::{Result, anyhow};
use beep_projects::issues::SourceError;
use beep_projects::issues::issue_source;
use beep_projects::{
    Shell,
    commands::{ChecksState, checks_of, gh_account, json, text},
    issues::run_rendered,
    model::IssueHost,
    quote,
};
use chrono::{DateTime, Utc};
use serde_json::{Deserializer, Value};
use std::time::SystemTime;

/// The most comments one read gives, newest kept.
const MAX_COMMENTS: usize = 50;
/// The most of the opening post one read gives.
const MAX_BODY: usize = 16 * 1024;
/// The most of one comment one read gives.
const MAX_COMMENT: usize = 4 * 1024;

const GH_PR_FIELDS: &str = "number,title,body,url,author,state,isDraft,labels,createdAt,comments,reviews,reviewDecision,statusCheckRollup";
const GH_ISSUE_FIELDS: &str = "number,title,body,url,author,state,labels,createdAt,comments";

/// Reads thread `number` of `project` from its host now.
pub(crate) async fn read(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    number: u64,
) -> Result<ThreadDetail, IssueError> {
    let Some(source) = issue_source(&StoredProject::new(user, project), fleet).await? else {
        return Err(IssueError::from(SourceError::Refused(
            "The project has no GitHub or Forgejo host.".to_owned(),
        )));
    };
    let failed = |error: anyhow::Error| {
        IssueError::from(SourceError::Refused(format!(
            "Could not read the host: {error:#}"
        )))
    };
    let out = run_rendered(
        source.shell.as_ref() as &dyn Shell,
        &render(&source.host, &project.name.to_string(), number),
    )
    .await
    .map_err(failed)?;
    let detail = parse(&source.host, &out).map_err(failed)?;
    if detail.number != number {
        return Err(IssueError::from(SourceError::Refused(format!(
            "The host gave #{} for #{number}.",
            detail.number
        ))));
    }
    Ok(detail)
}

/// The shell command that prints thread `number` of `repo` with its comments.
fn render(host: &IssueHost, repo: &str, number: u64) -> String {
    let number = number.to_string();
    match host {
        IssueHost::Github => {
            let gh = |kind: &str, fields: &str| {
                let words = [kind, "view", &number, "-R", repo, "--json", fields];
                let parts: Vec<String> = words.iter().map(|word| quote(word)).collect();
                format!("{}gh {}", gh_account(repo), parts.join(" "))
            };
            // A number that is not a pull request is an issue.
            format!(
                "{} 2>/dev/null || {}",
                gh("pr", GH_PR_FIELDS),
                gh("issue", GH_ISSUE_FIELDS)
            )
        }
        IssueHost::Forgejo(host) => {
            let tea = |words: &[&str], flags: &[&str]| {
                let mut parts = vec!["teacli"];
                parts.extend(words);
                parts.extend([repo, number.as_str(), "--host", host.as_str()]);
                parts.extend(flags);
                parts.extend(["--format", "json"]);
                parts
                    .iter()
                    .map(|word| quote(word))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            // Forgejo keeps a pull request's conversation as its issue's comments.
            format!(
                "{} && {}",
                tea(&["issue", "get"], &[]),
                tea(&["issue", "comment", "list"], &["--all"])
            )
        }
    }
}

fn time(value: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_str))
        .find_map(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc))
}

/// gh's `author`, else Forgejo's `user`.
fn login(value: &Value) -> String {
    ["author", "user"]
        .iter()
        .filter_map(|key| value.get(*key))
        .map(|who| text(who, "login"))
        .find(|login| !login.is_empty())
        .unwrap_or_default()
}

fn labels(value: &Value) -> Vec<String> {
    value
        .get("labels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|label| text(label, "name"))
        .filter(|name| !name.is_empty())
        .collect()
}

/// A review without text says what it decided.
fn review_text(review: &Value) -> String {
    let body = text(review, "body");
    if !body.trim().is_empty() {
        return body;
    }
    match text(review, "state").as_str() {
        "APPROVED" => "Approved".to_owned(),
        "CHANGES_REQUESTED" => "Requested changes".to_owned(),
        _ => String::new(),
    }
}

/// The posts under `key` (`comments`, `reviews`, or the Forgejo list itself),
/// skipping empty ones.
fn posts<'v>(
    list: impl Iterator<Item = &'v Value>,
    review: bool,
) -> Vec<(DateTime<Utc>, ThreadPost)> {
    list.filter_map(|item| {
        let at = time(item, &["createdAt", "submittedAt", "created_at"])?;
        let body = if review {
            review_text(item)
        } else {
            text(item, "body")
        };
        if body.trim().is_empty() {
            return None;
        }
        Some((
            at,
            ThreadPost {
                author: login(item),
                at: at.into(),
                body: clipped(&body, MAX_COMMENT),
            },
        ))
    })
    .collect()
}

fn array(value: Option<&Value>) -> impl Iterator<Item = &Value> {
    value.and_then(Value::as_array).into_iter().flatten()
}

/// Parses [`render`]'s output: one gh document, or Forgejo's issue then its
/// comments.
fn parse(host: &IssueHost, out: &str) -> Result<ThreadDetail> {
    let docs: Vec<Value> = Deserializer::from_str(out.trim())
        .into_iter::<Value>()
        .collect::<Result<_, _>>()
        .or_else(|_| json(out).map(|value| vec![value]))?;
    let item = docs
        .first()
        .ok_or_else(|| anyhow!("the host printed nothing"))?;
    let number = item
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("the thread has no number"))?;
    let state = text(item, "state").to_ascii_lowercase();
    let mut posts = match host {
        IssueHost::Github => {
            let mut all = posts(array(item.get("comments")), false);
            all.extend(posts(array(item.get("reviews")), true));
            all
        }
        IssueHost::Forgejo(_) => posts(array(docs.get(1)), false),
    };
    posts.sort_by_key(|(at, _)| *at);
    let earlier = posts.len().saturating_sub(MAX_COMMENTS);
    let (kind, merged, draft, review, checks) = match host {
        IssueHost::Github => {
            let pr = text(item, "url").contains("/pull/");
            let review = match text(item, "reviewDecision").as_str() {
                "APPROVED" => GitReview::Approved {},
                "CHANGES_REQUESTED" => GitReview::ChangesRequested {},
                _ => GitReview::None {},
            };
            let checks = match checks_of(item.get("statusCheckRollup")) {
                ChecksState::Failing => Some(RunState::Failed {}),
                ChecksState::Pending => Some(RunState::Running {}),
                ChecksState::Passing => Some(RunState::Passed {}),
                ChecksState::None => None,
            };
            (
                pr,
                state == "merged",
                item.get("isDraft").and_then(Value::as_bool) == Some(true),
                review,
                checks,
            )
        }
        IssueHost::Forgejo(_) => {
            let pr = item.get("pull_request").filter(|pr| !pr.is_null());
            let flag =
                |key: &str| pr.and_then(|pr| pr.get(key)).and_then(Value::as_bool) == Some(true);
            (
                pr.is_some(),
                flag("merged"),
                flag("draft"),
                GitReview::None {},
                None,
            )
        }
    };
    let opened =
        time(item, &["createdAt", "created_at"]).map_or(SystemTime::UNIX_EPOCH, Into::into);
    Ok(ThreadDetail {
        kind: if kind {
            GitItemKind::PullRequest {}
        } else {
            GitItemKind::Issue {}
        },
        number,
        title: text(item, "title").trim().to_owned(),
        url: ["html_url", "url"]
            .iter()
            .map(|key| text(item, key))
            .find(|url| !url.is_empty())
            .unwrap_or_default(),
        author: login(item),
        state: if merged {
            ThreadState::Merged {}
        } else if state == "open" {
            ThreadState::Open {}
        } else {
            ThreadState::Closed {}
        },
        draft,
        labels: labels(item),
        opened_at: opened,
        body: clipped(&text(item, "body"), MAX_BODY),
        comments: posts
            .into_iter()
            .skip(earlier)
            .map(|(_, post)| post)
            .collect(),
        earlier: u32::try_from(earlier).unwrap_or(u32::MAX),
        review,
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_projects::model::ForgejoHost;
    use serde_json::json;

    #[test]
    fn a_github_pull_request_reads_with_reviews_and_checks() {
        let out = json!({
            "number": 41, "title": " Cut the cache ", "body": "Shorter **now**", "url": "https://github.com/acme/widget/pull/41",
            "author": {"login": "sam"}, "state": "OPEN", "isDraft": false,
            "labels": [{"name": "perf"}], "createdAt": "2026-10-30T09:00:00Z",
            "comments": [{"author": {"login": "alice"}, "createdAt": "2026-10-30T11:00:00Z", "body": "<img src=x onerror=alert(1)>"}],
            "reviews": [{"author": {"login": "kim"}, "submittedAt": "2026-10-30T10:00:00Z", "body": "", "state": "CHANGES_REQUESTED"}],
            "reviewDecision": "CHANGES_REQUESTED",
            "statusCheckRollup": [{"conclusion": "FAILURE"}],
        })
        .to_string();
        let d = parse(&IssueHost::Github, &out).unwrap();
        assert_eq!(d.kind, GitItemKind::PullRequest {});
        assert_eq!(d.title, "Cut the cache");
        assert_eq!(d.state, ThreadState::Open {});
        assert_eq!(d.labels, ["perf"]);
        assert_eq!(d.review, GitReview::ChangesRequested {});
        assert_eq!(d.checks, Some(RunState::Failed {}));
        // Oldest first, the review between; text kept as the host wrote it.
        let said: Vec<_> = d
            .comments
            .iter()
            .map(|c| (c.author.as_str(), c.body.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("kim", "Requested changes"),
                ("alice", "<img src=x onerror=alert(1)>")
            ]
        );
    }

    #[test]
    fn a_forgejo_pull_request_reads_its_issue_comments() {
        let issue = json!({
            "number": 7, "title": "Fix", "body": "", "html_url": "https://git.example.com/acme/widget/pulls/7",
            "user": {"login": "sam"}, "state": "closed", "labels": [], "created_at": "2026-10-30T09:00:00Z",
            "pull_request": {"merged": true, "draft": false},
        });
        let comments = json!([{"user": {"login": "alice"}, "created_at": "2026-10-30T10:00:00Z", "body": "Thanks"}]);
        let host = IssueHost::Forgejo(ForgejoHost::parse("git.example.com").unwrap());
        let d = parse(&host, &format!("{issue}\n{comments}\n")).unwrap();
        assert_eq!(d.kind, GitItemKind::PullRequest {});
        assert_eq!(d.state, ThreadState::Merged {});
        assert_eq!(d.url, "https://git.example.com/acme/widget/pulls/7");
        assert_eq!(d.comments.len(), 1);
        assert_eq!(d.comments[0].author, "alice");
    }
}
