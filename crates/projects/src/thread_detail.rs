//! One pull request or issue read in full from the project's GitHub or Forgejo host, through a
//! machine that has the project, for the project page's detail pane.

use crate::{
    commands::{ChecksState, HostCommand, checks_of, json, text},
    issues::{
        Machines, ProjectStore, SourceError, clipped, host_failed, issue_source, issue_sources,
        read_any, run_host,
    },
    model::IssueHost,
};
use anyhow::{Result, anyhow};
use beep_proto::{GitItemKind, GitReview, RunState, ThreadDetail, ThreadPost, ThreadState};
use chrono::{DateTime, Utc};
use serde_json::{Deserializer, Value};
use std::time::SystemTime;

/// The most comments one read gives, newest kept.
const MAX_COMMENTS: usize = 50;
/// The most of the opening post one read gives.
const MAX_BODY: usize = 16 * 1024;
/// The most of one comment one read gives.
const MAX_COMMENT: usize = 4 * 1024;

/// Reads thread `number` of the project from its host now, trying each machine that has it.
pub async fn read(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
    number: u64,
) -> Result<ThreadDetail, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Err(SourceError::Refused(
            "The project has no GitHub or Forgejo host.".to_owned(),
        ));
    };
    let (_, detail) = read_any(
        machines,
        &sources,
        store.name(),
        HostCommand::ViewThread { number },
        |out| parse(&sources[0].host, out),
    )
    .await?;
    if detail.number != number {
        return Err(SourceError::Refused(format!(
            "The host gave #{} for #{number}.",
            detail.number
        )));
    }
    Ok(detail)
}

/// Posts `body` as the user's comment on pull request or issue `number`, through the first
/// machine that has the project. It is not tried again elsewhere, so it posts at most once.
pub async fn comment(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
    kind: GitItemKind,
    number: u64,
    body: &str,
) -> Result<(), SourceError> {
    let Some(source) = issue_source(store, machines).await? else {
        return Err(SourceError::Refused(
            "The project has no GitHub or Forgejo host.".to_owned(),
        ));
    };
    let command = match kind {
        GitItemKind::PullRequest {} => HostCommand::CommentPr { number, body },
        GitItemKind::Issue {} => HostCommand::CommentIssue { number, body },
    };
    run_host(source.shell.as_ref(), &source.host, store.name(), command)
        .await
        .map_err(|error| host_failed(&error))?;
    Ok(())
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

/// Parses [`HostCommand::ViewThread`]'s output: one gh document, or Forgejo's issue then its
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
#[path = "tests/thread_detail.rs"]
mod tests;
