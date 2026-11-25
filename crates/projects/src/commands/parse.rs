//! Reads what `gh` and `tea` print: issues, pull requests, checks and logins.

use anyhow::{Context, Result, anyhow, bail};
use beep_proto::PullRequestHead;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub fn json(out: &str) -> Result<Value> {
    serde_json::from_str(out.trim()).context("the host did not print JSON")
}

/// The items of every page of a list, as `gh api --paginate` prints them:
/// one JSON array after another. teacli prints `null` for an empty one.
pub(crate) fn pages(out: &str, what: &str) -> Result<Vec<Value>> {
    let mut items = Vec::new();
    let mut read = false;

    for page in serde_json::Deserializer::from_str(out).into_iter::<Value>() {
        let page = page.context("the host did not print JSON")?;
        items.extend_from_slice(list_of(&page, what)?);
        read = true;
    }

    match read {
        true => Ok(items),
        false => bail!("the host did not print JSON"),
    }
}

/// The items of a list. teacli prints `null` for an empty one.
fn list_of<'a>(value: &'a Value, what: &str) -> Result<&'a [Value]> {
    if value.is_null() {
        return Ok(&[]);
    }

    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("expected a list of {what}"))
}

pub fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// An open issue on the host, by anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenIssue {
    pub number: u64,
    pub title: String,
    pub labels: Vec<String>,
    /// The login of who opened it.
    pub author: String,
    pub opened_at: DateTime<Utc>,
    pub comments: u32,
    pub url: String,
    /// A pull request is linked to it, to close it.
    pub linked_pr: bool,
    /// The logins it is assigned to.
    pub assignees: Vec<String>,
}

/// The issue's web page: Forgejo's `url` is its API address.
pub(super) fn web_url(item: &Value) -> String {
    ["html_url", "url"]
        .iter()
        .map(|key| text(item, key))
        .find(|url| !url.is_empty())
        .unwrap_or_default()
}

/// A login: gh's jq gives it as text, Forgejo as the `user` object.
pub(super) fn login(item: &Value) -> String {
    match item.get("author") {
        Some(Value::String(login)) => login.clone(),
        Some(author) => text(author, "login"),
        None => item
            .get("user")
            .map(|user| text(user, "login"))
            .unwrap_or_default(),
    }
}

/// gh lists the comments, Forgejo counts them.
pub(super) fn comment_count(item: &Value) -> u32 {
    let count = match item.get("comments") {
        Some(Value::Array(list)) => list.len() as u64,
        Some(other) => other.as_u64().unwrap_or(0),
        None => 0,
    };
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The label names of an issue or pull request: gh's jq gives names,
/// Forgejo label objects.
pub(super) fn label_names(item: &Value) -> Vec<String> {
    match item.get("labels") {
        Some(Value::Array(labels)) => labels
            .iter()
            .map(|label| match label {
                Value::String(name) => name.clone(),
                other => text(other, "name"),
            })
            .filter(|name| !name.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// The assignee logins of an issue: gh's jq gives logins, Forgejo and the GitHub API user objects.
pub(super) fn assignee_logins(item: &Value) -> Vec<String> {
    match item.get("assignees") {
        Some(Value::Array(assignees)) => assignees
            .iter()
            .map(|assignee| match assignee {
                Value::String(login) => login.clone(),
                other => text(other, "login"),
            })
            .filter(|login| !login.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Parses [`HostCommand::ListOpenIssues`], one page or several, skipping the pull requests
/// Forgejo lists with issues.
pub fn parse_open_issues(out: &str) -> Result<Vec<OpenIssue>> {
    let items = pages(out, "issues")?;
    items
        .iter()
        .filter(|item| item.get("pull_request").is_none_or(Value::is_null))
        .map(|item| {
            let number = item
                .get("number")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("an issue has no number"))?;
            let opened = ["createdAt", "created_at"]
                .iter()
                .map(|key| text(item, key))
                .find(|at| !at.is_empty())
                .ok_or_else(|| anyhow!("issue #{number} has no creation time"))?;
            let opened_at = DateTime::parse_from_rfc3339(&opened)
                .with_context(|| format!("issue #{number} has a bad creation time"))?
                .with_timezone(&Utc);
            Ok(OpenIssue {
                number,
                title: text(item, "title"),
                labels: label_names(item),
                author: login(item),
                opened_at,
                comments: comment_count(item),
                url: web_url(item),
                linked_pr: false,
                assignees: assignee_logins(item),
            })
        })
        .collect()
}

/// An open pull request on the host, by anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPr {
    pub number: u64,
    pub title: String,
    /// Its web page.
    pub url: String,
    /// The login of who opened it.
    pub author: String,
    pub draft: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The branch it is made from; empty when the host's CLI does not say.
    pub head: String,
    pub labels: Vec<String>,
}

/// The time in the first of `keys` that holds one; `what` names it in the error.
pub(super) fn time_of(
    item: &Value,
    keys: &[&str],
    what: &str,
    number: u64,
) -> Result<DateTime<Utc>> {
    let at = keys
        .iter()
        .map(|key| text(item, key))
        .find(|at| !at.is_empty())
        .ok_or_else(|| anyhow!("#{number} has no {what}"))?;
    Ok(DateTime::parse_from_rfc3339(&at)
        .with_context(|| format!("#{number} has a bad {what}"))?
        .with_timezone(&Utc))
}

/// Parses [`HostCommand::ListOpenPrs`] from gh (camelCase) or Forgejo (snake_case), every
/// page of it.
pub fn parse_open_prs(out: &str) -> Result<Vec<OpenPr>> {
    let items = pages(out, "pull requests")?;
    items
        .iter()
        .map(|item| {
            let number = item
                .get("number")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("a pull request has no number"))?;
            let created_at = time_of(item, &["createdAt", "created_at"], "creation time", number)?;
            // Optional: the list still reads without it.
            let updated_at = time_of(item, &["updatedAt", "updated_at"], "update time", number)
                .unwrap_or(created_at);
            let draft = ["isDraft", "draft"]
                .iter()
                .any(|key| item.get(key).and_then(Value::as_bool) == Some(true));
            let head = match item.get("head") {
                Some(Value::String(branch)) => branch.clone(),
                Some(head) => text(head, "ref"),
                None => text(item, "headRefName"),
            };
            Ok(OpenPr {
                number,
                title: text(item, "title"),
                url: web_url(item),
                author: login(item),
                draft,
                created_at,
                updated_at,
                head,
                labels: label_names(item),
            })
        })
        .collect()
}

/// Parses [`HostCommand::ViewPrHead`] from gh or Forgejo (a deleted fork's repo is null).
pub fn parse_pr_head(out: &str) -> Result<PullRequestHead> {
    let item = json(out)?;
    let number = item
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("the pull request has no number"))?;
    let (branch, fork) = match item.get("head") {
        Some(head) => {
            let repo = |side: Option<&Value>| {
                side.and_then(|side| side.get("repo"))
                    .map(|repo| text(repo, "full_name"))
                    .filter(|name| !name.is_empty())
            };
            let from = repo(Some(head));
            let into = repo(item.get("base"));
            let fork = match (from, into) {
                (Some(from), Some(into)) => !from.eq_ignore_ascii_case(&into),
                (None, _) => true,
                (Some(_), None) => false,
            };
            (text(head, "ref"), fork)
        }
        None => (
            text(&item, "headRefName"),
            item.get("isCrossRepository").and_then(Value::as_bool) == Some(true),
        ),
    };

    if branch.is_empty() {
        bail!("#{number} names no branch");
    }

    Ok(PullRequestHead {
        number,
        branch,
        fork,
    })
}

/// Parses [`HostCommand::WhoAmI`]: bare from gh, JSON from teacli.
pub fn parse_login(out: &str) -> Result<String> {
    let out = out.trim();
    let login = match serde_json::from_str::<Value>(out) {
        Ok(Value::String(login)) => login,
        Ok(user @ Value::Object(_)) => ["login", "username"]
            .iter()
            .map(|key| text(&user, key))
            .find(|login| !login.is_empty())
            .unwrap_or_default(),
        _ => out.to_owned(),
    };

    if login.is_empty() || login.contains(char::is_whitespace) {
        bail!("the host did not print a login");
    }

    Ok(login)
}

/// Parses [`HostCommand::CanPush`]: a bare `true` or `false` from gh, the repository's JSON from
/// teacli.
pub fn parse_can_push(out: &str) -> Result<bool> {
    let out = out.trim();

    match serde_json::from_str::<Value>(out) {
        Ok(Value::Bool(push)) => Ok(push),
        Ok(repo @ Value::Object(_)) => {
            let can = repo.get("permissions");
            let flag = |key| can.and_then(|can| can.get(key)).and_then(Value::as_bool);

            match (flag("push"), flag("admin")) {
                (None, None) => bail!("the host did not print permissions"),
                (push, admin) => Ok(push.unwrap_or(false) || admin.unwrap_or(false)),
            }
        }
        _ => bail!("the host did not print a permission"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksState {
    Pending,
    Passing,
    Failing,
    /// No checks are reported, or the host does not say.
    None,
}

/// Failing wins over pending, which wins over passing.
pub fn checks_of(rollup: Option<&Value>) -> ChecksState {
    let Some(checks) = rollup
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
    else {
        return ChecksState::None;
    };
    let mut pending = false;

    for check in checks {
        // A check run has `conclusion` and `status`; a commit status has `state`.
        let verdict = ["conclusion", "state"]
            .iter()
            .map(|key| text(check, key).to_ascii_uppercase())
            .find(|verdict| !verdict.is_empty());
        match verdict.as_deref() {
            Some(
                "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
                | "STARTUP_FAILURE",
            ) => {
                return ChecksState::Failing;
            }
            Some("SUCCESS" | "NEUTRAL" | "SKIPPED") => {}
            _ => pending = true,
        }
    }

    if pending {
        ChecksState::Pending
    } else {
        ChecksState::Passing
    }
}
