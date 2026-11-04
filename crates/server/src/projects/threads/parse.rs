//! Issue and pull request threads as the hosts' CLIs print them.

use super::{LAST_LINE, ThreadKind};
use anyhow::{Result, anyhow};
use beep_projects::{
    commands::{json, text},
    issues::head,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// One comment or review on a thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadComment {
    /// The writer's login.
    pub(crate) author: String,
    pub(crate) at: DateTime<Utc>,
    pub(crate) body: String,
}

/// Whether `login` is a bot: `app/`, `[bot]` or the usual `-bot` names.
pub(super) fn is_bot(login: &str) -> bool {
    let login = login.to_ascii_lowercase();
    login.ends_with("[bot]")
        || login.ends_with("-bot")
        || login.starts_with("app/")
        || matches!(login.as_str(), "github-actions" | "dependabot" | "renovate")
}

pub(super) fn is_me(login: &str, me: &str) -> bool {
    !me.is_empty() && login.eq_ignore_ascii_case(me)
}

/// Whether someone other than `me` (bots aside) wrote after `me`'s newest comment or review.
pub(crate) fn needs_reply(comments: &[ThreadComment], me: &str) -> bool {
    let newest = |mine: bool| {
        comments
            .iter()
            .filter(|c| !is_bot(&c.author) && is_me(&c.author, me) == mine)
            .map(|c| c.at)
            .max()
    };
    match (newest(false), newest(true)) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(other), Some(mine)) => other > mine,
    }
}

/// "sam: Can we cut it down?": the newest non-bot comment by someone else, on one line.
pub(super) fn last_other(comments: &[ThreadComment], me: &str) -> String {
    let Some(last) = comments
        .iter()
        .filter(|c| !is_bot(&c.author) && !is_me(&c.author, me))
        .max_by_key(|c| c.at)
    else {
        return String::new();
    };
    let line = last.body.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = head(&line, LAST_LINE);
    let more = if cut.len() < line.len() { "..." } else { "" };
    if cut.is_empty() {
        format!("{} reviewed", last.author)
    } else {
        format!("{}: {cut}{more}", last.author)
    }
}

/// One thread as the host printed it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Thread {
    pub(crate) kind: ThreadKind,
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) url: String,
    /// Who opened it.
    pub(crate) author: String,
    pub(crate) updated_at: Option<DateTime<Utc>>,
    /// Comments and reviews, oldest first.
    pub(crate) comments: Vec<ThreadComment>,
}

/// One of the user's open threads, as the list gives it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListedThread {
    pub(crate) kind: ThreadKind,
    pub(crate) repo: String,
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) last_other: String,
    pub(crate) needs_reply: bool,
    pub(crate) updated_at: DateTime<Utc>,
}

pub(super) fn login(value: &Value) -> String {
    value
        .get("author")
        .map(|author| text(author, "login"))
        .unwrap_or_default()
}

pub(super) fn time(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Utc))
}

/// A thread from `gh pr view`/`gh issue view` output; a PR's URL has `/pull/`.
pub(super) fn thread_from(value: &Value) -> Result<Thread> {
    let number = value
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("the thread has no number"))?;
    let url = text(value, "url");
    let kind = if url.contains("/pull/") || value.get("reviews").is_some() {
        ThreadKind::PullRequest
    } else {
        ThreadKind::Issue
    };
    let mut comments: Vec<ThreadComment> = Vec::new();
    for (key, at) in [("comments", "createdAt"), ("reviews", "submittedAt")] {
        for item in value
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(when) = time(item, at) else { continue };
            comments.push(ThreadComment {
                author: login(item),
                at: when,
                body: text(item, "body"),
            });
        }
    }
    comments.sort_by_key(|c| c.at);
    Ok(Thread {
        kind,
        number,
        title: text(value, "title"),
        body: text(value, "body"),
        url,
        author: login(value),
        updated_at: time(value, "updatedAt"),
        comments,
    })
}

pub(crate) fn parse_thread(out: &str) -> Result<Thread> {
    thread_from(&json(out)?)
}

pub(super) fn parse_threads(out: &str) -> Result<Vec<Thread>> {
    json(out)?
        .as_array()
        .ok_or_else(|| anyhow!("the host did not print a list"))?
        .iter()
        .map(thread_from)
        .collect()
}
