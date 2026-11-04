//! Shell commands for a project's git and host work, and parsers for their output. Every
//! argument is quoted; check host-bound text with [`HostCommand::leaks`] first.

mod host;
mod parse;
pub(crate) use host::{HostCommand, InlineComment, LABEL_BLOCKED, LABEL_REVIEW, LABEL_RUNNING};
pub(crate) use parse::{
    ChecksState, OpenIssue, OpenPr, OwnedIssue, PrHead, PrState, ViewedIssue, parse_created_issue,
    parse_found_issue, parse_login, parse_open_issues, parse_open_prs, parse_owned_issues,
    parse_pr, parse_pr_head, parse_viewed_issue, parse_visibility, pr_fingerprint,
};
pub(super) use parse::{checks_of, json, text};

use super::model::{IssueHost, MachinePath, RepoName, Roles, TaskSlug, remote_parts};
use crate::{account::credentials::github_owner_var, nodes::Online};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// Most output a command keeps.
const MAX_OUTPUT: usize = 64 * 1024;

/// Where a project's commands run.
#[async_trait]
pub(crate) trait Shell: Send + Sync {
    async fn run(&self, command: &str, limit: Duration) -> Result<CommandOutput>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutput {
    /// `None` when the command did not finish.
    pub(crate) code: Option<i32>,
    /// The first 64 KiB of what it printed.
    pub(crate) out: String,
}

#[async_trait]
impl Shell for Online {
    async fn run(&self, command: &str, limit: Duration) -> Result<CommandOutput> {
        let mut out = String::new();
        let exited = self
            .exec(command, limit, &mut |text| push_capped(&mut out, text))
            .await?;
        Ok(CommandOutput {
            code: exited.exit_code,
            out,
        })
    }
}

fn push_capped(out: &mut String, text: &str) {
    let room = MAX_OUTPUT.saturating_sub(out.len());
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&text[..end]);
}

/// `text` as one shell word.
pub(crate) fn quote(text: &str) -> String {
    // Only a NUL byte cannot be quoted, and no command can carry one.
    shlex::try_quote(&text.replace('\0', ""))
        .map(|quoted| quoted.into_owned())
        .unwrap_or_default()
}

// ---- git ----

/// Keeps `.worktrees/` and `.beep/` out of `git status` in the checkout.
pub(crate) fn exclude_project_dirs(checkout: &MachinePath) -> String {
    let checkout = quote(checkout.as_str());
    format!(
        "exclude=\"$(git -C {checkout} rev-parse --path-format=absolute --git-common-dir)/info/exclude\" \
         && mkdir -p \"$(dirname \"$exclude\")\" \
         && for entry in .worktrees/ .beep/; do \
         grep -qxF \"$entry\" \"$exclude\" 2>/dev/null || echo \"$entry\" >> \"$exclude\"; done"
    )
}

/// Makes the task's worktree on a new `beep/<slug>` branch from `origin`'s `base`, or the
/// local one when it has commits `origin` lacks.
pub(crate) fn worktree_add(checkout: &MachinePath, slug: &TaskSlug, base: &str) -> String {
    let repo = quote(checkout.as_str());
    let local = quote(&format!("refs/heads/{base}"));
    let remote = quote(&format!("refs/remotes/origin/{base}"));
    format!(
        "git -C {repo} fetch origin {base_word}; \
         if git -C {repo} rev-parse -q --verify {local} >/dev/null \
         && ! git -C {repo} merge-base --is-ancestor {local} {remote} 2>/dev/null; \
         then start={local}; else start={origin}; fi; \
         git -C {repo} worktree add -b {branch} {path} \"$start\"",
        base_word = quote(base),
        branch = quote(&slug.branch()),
        path = quote(slug.worktree(checkout).as_str()),
        origin = quote(&format!("origin/{base}")),
    )
}

/// Prints the commit the checkout is at.
pub(crate) fn head_commit(checkout: &MachinePath) -> String {
    format!("git -C {} rev-parse HEAD", quote(checkout.as_str()))
}

/// Removes the task's worktree and its `beep/<slug>` branch, nothing else.
pub(crate) fn worktree_remove(checkout: &MachinePath, slug: &TaskSlug) -> String {
    let repo = quote(checkout.as_str());
    format!(
        "git -C {repo} worktree remove --force {} && git -C {repo} branch -D {}",
        quote(slug.worktree(checkout).as_str()),
        quote(&slug.branch()),
    )
}

const REMOTES_MARK: &str = "== remotes";
const CONFIG_MARK: &str = "== config";
const HEAD_MARK: &str = "== head";

/// Prints the checkout's raw remote URLs, its `beep.issues`/`beep.prs` settings and default branch.
pub(crate) fn read_setup(checkout: &MachinePath) -> String {
    let repo = quote(checkout.as_str());
    format!(
        "echo '{REMOTES_MARK}'; git -C {repo} config --get-regexp '^remote\\..*\\.url$'; \
         echo '{CONFIG_MARK}'; echo \"issues=$(git -C {repo} config --get beep.issues)\"; \
         echo \"prs=$(git -C {repo} config --get beep.prs)\"; \
         echo '{HEAD_MARK}'; git -C {repo} symbolic-ref --short refs/remotes/origin/HEAD"
    )
}

/// What `read_setup` found out about a checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Setup {
    pub(crate) roles: Roles,
    pub(crate) default_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SetupError {
    #[error("No remote of this checkout is {0}.")]
    OtherRepo(String),
    #[error("beep.issues and beep.prs are github, forgejo, forgejo:<host> or none, not {0:?}.")]
    BadConfig(String),
    #[error("The remote host {0:?} is not a hostname.")]
    BadHost(String),
}

/// Reads the output of [`read_setup`] for the project `name`.
pub(crate) fn parse_setup(out: &str, name: &RepoName) -> Result<Setup, SetupError> {
    let mut section = "";
    let mut remotes = Vec::new();
    let mut issues = "";
    let mut prs = "";
    let mut head = "";
    for line in out.lines().map(str::trim) {
        match line {
            REMOTES_MARK | CONFIG_MARK | HEAD_MARK => section = line,
            _ if section == REMOTES_MARK => remotes.push(line),
            _ if section == CONFIG_MARK => {
                if let Some(value) = line.strip_prefix("issues=") {
                    issues = value.trim();
                } else if let Some(value) = line.strip_prefix("prs=") {
                    prs = value.trim();
                }
            }
            _ if section == HEAD_MARK && !line.is_empty() => head = line,
            _ => {}
        }
    }
    let host = remotes
        .iter()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .filter_map(|(_, url)| remote_host(url.trim(), name))
        .next()
        .ok_or_else(|| SetupError::OtherRepo(name.to_string()))?;
    let roles = Roles {
        issues: role(issues, &host)?,
        prs: role(prs, &host)?,
    };
    let default_branch = head.strip_prefix("origin/").unwrap_or(head);
    Ok(Setup {
        roles,
        default_branch: if default_branch.is_empty() {
            "main".to_owned()
        } else {
            default_branch.to_owned()
        },
    })
}

/// The host of `url` when its path ends in `name` (optional `.git`, any case).
fn remote_host(url: &str, name: &RepoName) -> Option<String> {
    let (host, path) = remote_parts(url)?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let path = format!("/{path}").to_ascii_lowercase();
    let tail = format!("/{name}").to_ascii_lowercase();
    path.ends_with(&tail).then(|| host.to_ascii_lowercase())
}

/// Prefix that makes `gh` run as the user's account for `repo`'s owner (see
/// `credentials::for_machines`). Only the variable's name is in the command, never the token.
pub(crate) fn gh_account(repo: &str) -> String {
    repo.split_once('/')
        .and_then(|(owner, _)| github_owner_var(owner))
        .map(|var| format!("GH_TOKEN=\"${{{var}:-$GH_TOKEN}}\" "))
        .unwrap_or_default()
}

/// One role: what config says, else what the remote's host is.
fn role(configured: &str, remote_host: &str) -> Result<Option<IssueHost>, SetupError> {
    let from_host = || {
        IssueHost::of_hostname(remote_host)
            .map(Some)
            .ok_or_else(|| SetupError::BadHost(remote_host.to_owned()))
    };
    match configured {
        "" => from_host(),
        "none" => Ok(None),
        "github" => Ok(Some(IssueHost::Github)),
        "forgejo" => from_host().map(|host| host.filter(|host| *host != IssueHost::Github)),
        other => IssueHost::from_stored(other)
            .map(Some)
            .ok_or_else(|| SetupError::BadConfig(other.to_owned())),
    }
}

// ---- hosts ----

// ---- what the hosts print ----

#[cfg(test)]
mod tests;
