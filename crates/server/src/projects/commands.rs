//! Shell commands for the git and host work a project does on a machine,
//! and parsers for what they print. The builders only make text: running it
//! is [`Shell`]'s job. Every argument is quoted.
//!
//! Text that goes to a host (issue and comment bodies) should be checked with
//! [`HostCommand::leaks`] first.

use super::model::{
    ForgejoHost, IssueHost, IssueRef, MachinePath, RepoName, Roles, TaskSlug, Visibility,
};
use crate::{credentials::github_owner_var, leak::PublicText, nodes::Online};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;
use strum::IntoStaticStr;

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
fn quote(text: &str) -> String {
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

pub(crate) fn worktree_add(checkout: &MachinePath, slug: &TaskSlug, base: &str) -> String {
    let repo = quote(checkout.as_str());
    format!(
        "git -C {repo} fetch origin {} && git -C {repo} worktree add -b {} {} {}",
        quote(base),
        quote(&slug.branch()),
        quote(slug.worktree(checkout).as_str()),
        quote(&format!("origin/{base}")),
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

/// Prints the checkout's remotes (raw URLs, before `insteadOf` rewrites), its
/// `beep.issues` and `beep.prs` settings and the default branch.
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

/// The host of `url` when it is a remote of `name`: `owner/repo` with an
/// optional `.git`, ignoring case.
fn remote_host(url: &str, name: &RepoName) -> Option<String> {
    let (host, path) = match url.split_once("://") {
        Some((_, rest)) => {
            let (authority, path) = rest.split_once('/')?;
            let host = authority.rsplit('@').next()?;
            (host.split(':').next()?, path)
        }
        None => {
            let (authority, path) = url.split_once(':')?;
            (authority.rsplit('@').next()?, path)
        }
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    path.eq_ignore_ascii_case(&name.to_string())
        .then(|| host.to_ascii_lowercase())
}

/// What goes before a `gh` command for `repo` (`owner/repo`) so it runs as
/// the user's GitHub account for that owner: the machine holds that
/// account's token in the owner's var when it is not the default one
/// (see `credentials::for_machines`), and `GH_TOKEN` otherwise. Only the
/// variable's name is in the command, never the token.
fn gh_account(repo: &str) -> String {
    repo.split_once('/')
        .and_then(|(owner, _)| github_owner_var(owner))
        .map(|var| format!("GH_TOKEN=\"${{{var}:-$GH_TOKEN}}\" "))
        .unwrap_or_default()
}

/// One role: what config says, else what the remote's host is.
fn role(configured: &str, remote_host: &str) -> Result<Option<IssueHost>, SetupError> {
    let from_host = || {
        if remote_host == "github.com" {
            return Ok(Some(IssueHost::Github));
        }
        ForgejoHost::parse(remote_host)
            .map(|host| Some(IssueHost::Forgejo(host)))
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

/// The label every issue Beep files carries.
pub(crate) const LABEL_OWNED: &str = "beep";
pub(crate) const LABEL_RUNNING: &str = "beep:running";
pub(crate) const LABEL_BLOCKED: &str = "beep:blocked";
pub(crate) const LABEL_REVIEW: &str = "beep:review";

/// The labels [`HostCommand::EnsureLabels`] creates, with their colours.
const LABELS: [(&str, &str); 4] = [
    (LABEL_OWNED, "5319e7"),
    (LABEL_RUNNING, "0e8a16"),
    (LABEL_BLOCKED, "d93f0b"),
    (LABEL_REVIEW, "1d76db"),
];
const LABEL_DESCRIPTION: &str = "Managed by the project agent";

/// What Beep asks a code host for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HostCommand<'a> {
    EnsureLabels,
    CreateIssue {
        title: &'a str,
        body: &'a str,
    },
    Relabel {
        number: u64,
        add: Option<&'a str>,
        remove: &'a [&'a str],
    },
    EditIssue {
        number: u64,
        title: &'a str,
        body: &'a str,
    },
    /// Forgejo has no "not planned" reason, so `not_planned` only changes gh.
    CloseIssue {
        number: u64,
        comment: &'a str,
        not_planned: bool,
    },
    ReopenIssue {
        number: u64,
    },
    ListOwnedIssues,
    /// The newest open issues, any author's.
    ListOpenIssues,
    /// One issue with its text and author.
    ViewIssue {
        number: u64,
    },
    /// One comment on an issue.
    CommentIssue {
        number: u64,
        body: &'a str,
    },
    /// The login of the account the CLI is signed in as.
    WhoAmI,
    /// Beep's issues whose body holds `marker`, for one filed before.
    FindIssue {
        marker: &'a str,
    },
    FindPr {
        branch: &'a str,
        base: &'a str,
    },
    PrStatus {
        number: u64,
    },
    /// A squash merge.
    MergePr {
        number: u64,
    },
    Visibility,
}

/// The pieces of a command, quoted when joined.
struct Parts(Vec<String>);

impl Parts {
    fn new(words: &[&str]) -> Parts {
        Parts(words.iter().map(|word| quote(word)).collect())
    }

    fn with(mut self, words: &[&str]) -> Parts {
        self.0.extend(words.iter().map(|word| quote(word)));
        self
    }

    fn render(self) -> String {
        self.0.join(" ")
    }
}

impl HostCommand<'_> {
    /// The shell command that runs this against `repo` on `host`.
    pub(crate) fn render(&self, host: &IssueHost, repo: &RepoName) -> String {
        match host {
            IssueHost::Github => self.gh(&repo.to_string()),
            IssueHost::Forgejo(host) => self.teacli(host.as_str(), &repo.to_string()),
        }
    }

    /// The text this command sends to the host, for [`Self::leaks`].
    fn outgoing(&self) -> Vec<&str> {
        match self {
            Self::CreateIssue { title, body } | Self::EditIssue { title, body, .. } => {
                vec![title, body]
            }
            Self::CloseIssue { comment, .. } => vec![comment],
            Self::CommentIssue { body, .. } => vec![body],
            _ => Vec::new(),
        }
    }

    /// The private details in the text this command would publish, each once.
    pub(crate) fn leaks(&self, public_text: &PublicText) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for hit in self
            .outgoing()
            .into_iter()
            .flat_map(|text| public_text.hits(text))
        {
            if !found.contains(&hit) {
                found.push(hit);
            }
        }
        found
    }

    fn gh(&self, repo: &str) -> String {
        let account = gh_account(repo);
        let gh = |words: &[&str], flags: &[&str]| {
            let command = Parts::new(&["gh"])
                .with(words)
                .with(&["-R", repo])
                .with(flags)
                .render();
            format!("{account}{command}")
        };
        let number_command = |sub: &[&str], number: u64, flags: &[&str]| {
            let number = number.to_string();
            let mut words = sub.to_vec();
            words.push(&number);
            gh(&words, flags)
        };
        match *self {
            Self::EnsureLabels => LABELS
                .iter()
                .map(|(name, color)| {
                    gh(
                        &["label", "create", name],
                        &[
                            "--color",
                            color,
                            "--description",
                            LABEL_DESCRIPTION,
                            "--force",
                        ],
                    )
                })
                .collect::<Vec<_>>()
                .join(" && "),
            Self::CreateIssue { title, body } => gh(
                &["issue", "create"],
                &["--title", title, "--body", body, "--label", LABEL_OWNED],
            ),
            Self::Relabel {
                number,
                add,
                remove,
            } => {
                let mut flags = Vec::new();
                if let Some(add) = add {
                    flags.extend(["--add-label", add]);
                }
                for label in remove {
                    flags.extend(["--remove-label", label]);
                }
                if flags.is_empty() {
                    return "true".to_owned();
                }
                number_command(&["issue", "edit"], number, &flags)
            }
            Self::EditIssue {
                number,
                title,
                body,
            } => number_command(
                &["issue", "edit"],
                number,
                &["--title", title, "--body", body],
            ),
            Self::CloseIssue {
                number,
                comment,
                not_planned,
            } => {
                let mut flags = vec!["--comment", comment];
                if not_planned {
                    flags.extend(["--reason", "not planned"]);
                }
                number_command(&["issue", "close"], number, &flags)
            }
            Self::ReopenIssue { number } => number_command(&["issue", "reopen"], number, &[]),
            Self::ListOwnedIssues => gh(
                &["issue", "list"],
                &[
                    "--label",
                    LABEL_OWNED,
                    "--state",
                    "all",
                    "--limit",
                    "50",
                    "--json",
                    "number,state,title,comments",
                ],
            ),
            Self::ListOpenIssues => {
                // Only what the list shows, so a long list stays small.
                let jq = "[.[] | {number, title, labels: [.labels[].name], \
                          author: .author.login, createdAt, \
                          comments: (.comments | length), url}]";
                let limit = GH_OPEN_LIMIT.to_string();
                gh(
                    &["issue", "list"],
                    &[
                        "--state",
                        "open",
                        "--limit",
                        &limit,
                        "--json",
                        "number,title,labels,author,createdAt,comments,url",
                        "--jq",
                        jq,
                    ],
                )
            }
            Self::ViewIssue { number } => number_command(
                &["issue", "view"],
                number,
                &["--json", "number,title,body,url,state,author"],
            ),
            Self::CommentIssue { number, body } => {
                number_command(&["issue", "comment"], number, &["--body", body])
            }
            // `gh api` takes no -R: the account is the same for every repo.
            Self::WhoAmI => Parts::new(&["gh", "api", "user", "--jq", ".login"]).render(),
            Self::FindIssue { marker } => {
                // Filtered by gh itself, so only the match is printed.
                let jq = format!(
                    "[.[] | select(.body | contains({})) | {{number, url, body}}]",
                    Value::String(marker.to_owned())
                );
                gh(
                    &["issue", "list"],
                    &[
                        "--label",
                        LABEL_OWNED,
                        "--state",
                        "all",
                        "--limit",
                        "500",
                        "--json",
                        "number,url,body",
                        "--jq",
                        &jq,
                    ],
                )
            }
            Self::FindPr { branch, base } => gh(
                &["pr", "list"],
                &[
                    "--head", branch, "--base", base, "--state", "all", "--limit", "1", "--json",
                    PR_FIELDS,
                ],
            ),
            Self::PrStatus { number } => {
                number_command(&["pr", "view"], number, &["--json", PR_FIELDS])
            }
            Self::MergePr { number } => number_command(&["pr", "merge"], number, &["--squash"]),
            Self::Visibility => format!(
                "{account}{}",
                Parts::new(&["gh", "repo", "view", repo])
                    .with(&["--json", "visibility"])
                    .render()
            ),
        }
    }

    fn teacli(&self, host: &str, repo: &str) -> String {
        let tea = |words: &[&str], positional: &[&str], flags: &[&str]| {
            Parts::new(&["teacli"])
                .with(words)
                .with(&[repo])
                .with(positional)
                .with(&["--host", host])
                .with(flags)
                .with(&["--format", "json"])
                .render()
        };
        match *self {
            Self::EnsureLabels => LABELS
                .iter()
                .map(|(name, color)| {
                    let exists = format!(
                        "{} | grep -Eq {}",
                        tea(&["label", "list"], &[], &["--all"]),
                        quote(&format!("\"name\": ?\"{name}\""))
                    );
                    let create = tea(
                        &["label", "create"],
                        &[],
                        &["-n", name, "-c", color, "-d", LABEL_DESCRIPTION],
                    );
                    format!("{{ {exists} || {create}; }}")
                })
                .collect::<Vec<_>>()
                .join(" && "),
            Self::CreateIssue { title, body } => tea(
                &["issue", "create"],
                &[],
                &["-t", title, "-b", body, "--label", LABEL_OWNED],
            ),
            Self::Relabel {
                number,
                add,
                remove,
            } => {
                let number = number.to_string();
                let mut commands = Vec::new();
                if let Some(add) = add {
                    commands.push(tea(&["issue", "label", "add"], &[&number, add], &[]));
                }
                if !remove.is_empty() {
                    let mut positional = vec![number.as_str()];
                    positional.extend(remove);
                    commands.push(tea(&["issue", "label", "remove"], &positional, &[]));
                }
                if commands.is_empty() {
                    return "true".to_owned();
                }
                commands.join(" && ")
            }
            Self::EditIssue {
                number,
                title,
                body,
            } => tea(
                &["issue", "update"],
                &[&number.to_string()],
                &["-t", title, "-b", body],
            ),
            Self::CloseIssue {
                number, comment, ..
            } => {
                let number = number.to_string();
                format!(
                    "{} && {}",
                    tea(
                        &["issue", "comment", "create"],
                        &[&number],
                        &["-b", comment]
                    ),
                    tea(&["issue", "close"], &[&number], &[]),
                )
            }
            Self::ReopenIssue { number } => tea(&["issue", "reopen"], &[&number.to_string()], &[]),
            Self::ListOwnedIssues => tea(
                &["issue", "list"],
                &[],
                &["--label", LABEL_OWNED, "-s", "all", "--per-page", "50"],
            ),
            Self::ListOpenIssues => tea(
                &["issue", "list"],
                &[],
                &["-s", "open", "--per-page", &TEA_OPEN_LIMIT.to_string()],
            ),
            Self::ViewIssue { number } => tea(&["issue", "get"], &[&number.to_string()], &[]),
            Self::CommentIssue { number, body } => tea(
                &["issue", "comment", "create"],
                &[&number.to_string()],
                &["-b", body],
            ),
            Self::WhoAmI => Parts::new(&["teacli", "whoami", "--host", host])
                .with(&["--format", "json"])
                .render(),
            Self::FindIssue { marker } => tea(
                &["issue", "list"],
                &[],
                &[
                    "--label",
                    LABEL_OWNED,
                    "-s",
                    "all",
                    "-q",
                    marker,
                    "--per-page",
                    "50",
                ],
            ),
            Self::FindPr { branch, base } => tea(&["pr", "find"], &[base, branch], &[]),
            Self::PrStatus { number } => tea(&["pr", "get"], &[&number.to_string()], &[]),
            Self::MergePr { number } => tea(
                &["pr", "merge"],
                &[&number.to_string()],
                &["--method", "squash"],
            ),
            Self::Visibility => tea(&["repo", "get"], &[], &[]),
        }
    }
}

/// Most open issues one list reads. teacli prints each issue's whole text,
/// so it reads fewer to stay under the output cap.
pub(crate) const GH_OPEN_LIMIT: usize = 100;
pub(crate) const TEA_OPEN_LIMIT: usize = 25;

const PR_FIELDS: &str = "state,mergedAt,reviewDecision,statusCheckRollup,url,number,title,body";

// ---- what the hosts print ----

fn json(out: &str) -> Result<Value> {
    serde_json::from_str(out.trim()).context("the host did not print JSON")
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The issue a create command printed: gh prints its URL, teacli its JSON.
pub(crate) fn parse_created_issue(host: &IssueHost, out: &str) -> Result<IssueRef> {
    let (number, url) = match host {
        IssueHost::Github => {
            let url = out
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .ok_or_else(|| anyhow!("gh printed nothing"))?;
            let last = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
            let number = last
                .parse()
                .with_context(|| format!("not an issue URL: {url}"))?;
            (number, url.to_owned())
        }
        IssueHost::Forgejo(_) => {
            let value = json(out)?;
            let number = value
                .get("number")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("the issue has no number"))?;
            let url = ["html_url", "url"]
                .iter()
                .map(|key| text(&value, key))
                .find(|url| !url.is_empty())
                .unwrap_or_default();
            (number, url)
        }
    };
    Ok(IssueRef { number, url })
}

/// The issue [`HostCommand::FindIssue`] found holding `marker`, if any.
pub(crate) fn parse_found_issue(out: &str, marker: &str) -> Result<Option<IssueRef>> {
    if out.trim().is_empty() {
        return Ok(None);
    }
    let value = json(out)?;
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("expected a list of issues"))?;
    // The host's search may match loosely, so the body is checked here.
    let Some(item) = items
        .iter()
        .find(|item| text(item, "body").contains(marker))
    else {
        return Ok(None);
    };
    let number = item
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("the issue has no number"))?;
    let url = ["html_url", "url"]
        .iter()
        .map(|key| text(item, key))
        .find(|url| !url.is_empty())
        .unwrap_or_default();
    Ok(Some(IssueRef { number, url }))
}

/// An issue Beep filed, as the list command prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedIssue {
    pub(crate) number: u64,
    pub(crate) open: bool,
    pub(crate) title: String,
    pub(crate) comments: u32,
}

pub(crate) fn parse_owned_issues(_host: &IssueHost, out: &str) -> Result<Vec<OwnedIssue>> {
    let value = json(out)?;
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("expected a list of issues"))?;
    items
        .iter()
        .map(|item| {
            let number = item
                .get("number")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("an issue has no number"))?;
            Ok(OwnedIssue {
                number,
                open: text(item, "state").eq_ignore_ascii_case("open"),
                title: text(item, "title"),
                comments: comment_count(item),
            })
        })
        .collect()
}

/// An open issue on the host, by anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenIssue {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) labels: Vec<String>,
    /// The login of who opened it.
    pub(crate) author: String,
    pub(crate) opened_at: DateTime<Utc>,
    pub(crate) comments: u32,
    pub(crate) url: String,
}

/// One issue with its text, as [`HostCommand::ViewIssue`] prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewedIssue {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) url: String,
    pub(crate) open: bool,
    /// The login of who opened it.
    pub(crate) author: String,
}

/// The issue's web page: Forgejo's `url` is its API address.
fn web_url(item: &Value) -> String {
    ["html_url", "url"]
        .iter()
        .map(|key| text(item, key))
        .find(|url| !url.is_empty())
        .unwrap_or_default()
}

/// A login: gh's jq gives it as text, Forgejo as the `user` object.
fn login(item: &Value) -> String {
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
fn comment_count(item: &Value) -> u32 {
    let count = match item.get("comments") {
        Some(Value::Array(list)) => list.len() as u64,
        Some(other) => other.as_u64().unwrap_or(0),
        None => 0,
    };
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The open issues [`HostCommand::ListOpenIssues`] printed, leaving out
/// pull requests, which Forgejo lists with the issues.
pub(crate) fn parse_open_issues(out: &str) -> Result<Vec<OpenIssue>> {
    let value = json(out)?;
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("expected a list of issues"))?;
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
            let labels = match item.get("labels") {
                Some(Value::Array(labels)) => labels
                    .iter()
                    .map(|label| match label {
                        Value::String(name) => name.clone(),
                        other => text(other, "name"),
                    })
                    .filter(|name| !name.is_empty())
                    .collect(),
                _ => Vec::new(),
            };
            Ok(OpenIssue {
                number,
                title: text(item, "title"),
                labels,
                author: login(item),
                opened_at,
                comments: comment_count(item),
                url: web_url(item),
            })
        })
        .collect()
}

/// The issue [`HostCommand::ViewIssue`] printed. A pull request is refused.
pub(crate) fn parse_viewed_issue(out: &str) -> Result<ViewedIssue> {
    let item = json(out)?;
    if item.get("pull_request").is_some_and(|pr| !pr.is_null()) {
        bail!("that number is a pull request, not an issue");
    }
    let number = item
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("the issue has no number"))?;
    Ok(ViewedIssue {
        number,
        title: text(&item, "title"),
        body: text(&item, "body"),
        url: web_url(&item),
        open: text(&item, "state").eq_ignore_ascii_case("open"),
        author: login(&item),
    })
}

/// The login [`HostCommand::WhoAmI`] printed: gh prints it bare, teacli
/// prints the user as JSON.
pub(crate) fn parse_login(out: &str) -> Result<String> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ChecksState {
    Pending,
    Passing,
    Failing,
    /// No checks are reported, or the host does not say.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrState {
    pub(crate) number: u64,
    pub(crate) url: String,
    pub(crate) merged: bool,
    pub(crate) open: bool,
    /// `APPROVED`, `CHANGES_REQUESTED` or `REVIEW_REQUIRED`, when the host
    /// says. Forgejo's pull request JSON carries no review decision.
    pub(crate) review: Option<String>,
    /// Forgejo's pull request JSON carries no checks, so this is `None` there.
    pub(crate) checks: ChecksState,
    pub(crate) title: String,
    pub(crate) body: String,
}

/// A pull request, or `None` when the output is empty or a list with none.
pub(crate) fn parse_pr(host: &IssueHost, out: &str) -> Result<Option<PrState>> {
    if out.trim().is_empty() || out.trim() == "null" {
        return Ok(None);
    }
    let value = json(out)?;
    let item = match &value {
        Value::Array(list) => match list.first() {
            Some(first) => first,
            None => return Ok(None),
        },
        other => other,
    };
    if item.get("error").and_then(Value::as_bool) == Some(true) {
        bail!("the host said: {}", text(item, "message"));
    }
    let number = item
        .get("number")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("the pull request has no number"))?;
    let state = text(item, "state");
    let open = state.eq_ignore_ascii_case("open");
    let (url, merged, review, checks) = match host {
        IssueHost::Github => (
            text(item, "url"),
            state.eq_ignore_ascii_case("merged")
                || item.get("mergedAt").is_some_and(|at| !at.is_null()),
            Some(text(item, "reviewDecision")).filter(|review| !review.is_empty()),
            checks_of(item.get("statusCheckRollup")),
        ),
        IssueHost::Forgejo(_) => (
            text(item, "html_url"),
            item.get("merged").and_then(Value::as_bool).unwrap_or(false),
            None,
            ChecksState::None,
        ),
    };
    Ok(Some(PrState {
        number,
        url,
        merged,
        open,
        review,
        checks,
        title: text(item, "title"),
        body: text(item, "body"),
    }))
}

/// Failing wins over pending, which wins over passing.
fn checks_of(rollup: Option<&Value>) -> ChecksState {
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

/// Whether the repository is public. Anything unreadable counts as public,
/// which is the cautious answer for what may be said in it.
pub(crate) fn parse_visibility(_host: &IssueHost, out: &str) -> Visibility {
    let Ok(value) = json(out) else {
        return Visibility::Public;
    };
    let private = match value.get("visibility").and_then(Value::as_str) {
        Some(visibility) => matches!(
            visibility.to_ascii_lowercase().as_str(),
            "private" | "internal"
        ),
        None => value.get("private").and_then(Value::as_bool) == Some(true),
    };
    if private {
        Visibility::Private
    } else {
        Visibility::Public
    }
}

/// What changed about a pull request, for telling a new state from an old one.
pub(crate) fn pr_fingerprint(pr: &PrState) -> String {
    let state = if pr.open { "open" } else { "closed" };
    let checks: &'static str = pr.checks.into();
    format!(
        "{state}|{}|{}|{checks}",
        pr.merged,
        pr.review.as_deref().unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> RepoName {
        RepoName::parse("acme/widget").expect("a repo")
    }

    fn github() -> IssueHost {
        IssueHost::Github
    }

    fn forgejo() -> IssueHost {
        IssueHost::Forgejo(ForgejoHost::parse("git.example.com").expect("a host"))
    }

    #[test]
    fn worktree_add_quotes_paths() {
        let checkout = MachinePath::parse("/srv/my project").expect("a path");
        let slug = TaskSlug::parse("fix-it").expect("a slug");
        let command = worktree_add(&checkout, &slug, "main");
        assert_eq!(
            command,
            "git -C '/srv/my project' fetch origin main && git -C '/srv/my project' \
             worktree add -b beep/fix-it '/srv/my project/.worktrees/fix-it' origin/main"
        );
        let remove = worktree_remove(&checkout, &slug);
        assert_eq!(
            remove,
            "git -C '/srv/my project' worktree remove --force \
             '/srv/my project/.worktrees/fix-it' && git -C '/srv/my project' branch -D beep/fix-it"
        );
        let hostile = worktree_add(&checkout, &slug, "a; rm -rf /");
        assert!(hostile.contains("'a; rm -rf /'"), "{hostile}");
    }

    #[test]
    fn exclude_and_setup_commands_name_the_checkout_once_per_use() {
        let checkout = MachinePath::parse("/srv/my project").expect("a path");
        assert!(exclude_project_dirs(&checkout).contains("git -C '/srv/my project' rev-parse"));
        assert_eq!(
            read_setup(&checkout).matches("'/srv/my project'").count(),
            4
        );
    }

    fn setup_out(remote: &str, issues: &str, prs: &str, head: &str) -> String {
        format!(
            "== remotes\nremote.origin.url {remote}\n== config\nissues={issues}\nprs={prs}\n\
             == head\n{head}\n"
        )
    }

    #[test]
    fn setup_reads_roles_from_config_then_remotes() {
        let out = setup_out(
            "https://git.example.com/Acme/Widget",
            "forgejo",
            "",
            "origin/trunk",
        );
        let setup = parse_setup(&out, &repo()).expect("a setup");
        assert_eq!(setup.roles.issues, Some(forgejo()));
        assert_eq!(setup.roles.prs, Some(forgejo()));
        assert_eq!(setup.default_branch, "trunk");

        let out = setup_out("git@github.com:acme/widget.git", "", "none", "");
        let setup = parse_setup(&out, &repo()).expect("a setup");
        assert_eq!(setup.roles.issues, Some(github()));
        assert_eq!(setup.roles.prs, None);
        assert_eq!(setup.default_branch, "main");

        let out = setup_out(
            "ssh://git@git.example.com:2222/acme/widget.git",
            "github",
            "",
            "",
        );
        let setup = parse_setup(&out, &repo()).expect("a setup");
        assert_eq!(setup.roles.issues, Some(github()));
        assert_eq!(setup.roles.prs, Some(forgejo()));
    }

    #[test]
    fn setup_refuses_another_repo() {
        let out = setup_out("git@github.com:acme/other.git", "", "", "origin/main");
        assert_eq!(
            parse_setup(&out, &repo()),
            Err(SetupError::OtherRepo("acme/widget".into()))
        );
        let out = setup_out("git@github.com:acme/widget.git", "svn", "", "");
        assert_eq!(
            parse_setup(&out, &repo()),
            Err(SetupError::BadConfig("svn".into()))
        );
    }

    #[test]
    fn open_issue_commands_render() {
        let list = HostCommand::ListOpenIssues.render(&github(), &repo());
        assert!(
            list.starts_with(
                "gh issue list -R acme/widget --state open --limit 100 \
                 --json 'number,title,labels,author,createdAt,comments,url' --jq"
            ),
            "{list}"
        );
        assert_eq!(
            HostCommand::ListOpenIssues.render(&forgejo(), &repo()),
            "teacli issue list acme/widget --host git.example.com -s open --per-page 25 \
             --format json"
        );
        assert_eq!(
            HostCommand::ViewIssue { number: 9 }.render(&github(), &repo()),
            "gh issue view 9 -R acme/widget --json 'number,title,body,url,state,author'"
        );
        assert_eq!(
            HostCommand::ViewIssue { number: 9 }.render(&forgejo(), &repo()),
            "teacli issue get acme/widget 9 --host git.example.com --format json"
        );
    }

    #[test]
    fn open_issues_parse_from_gh_and_teacli() {
        let gh = r#"[{"number":12,"title":"Crash on start","labels":["bug"],
            "author":"sam","createdAt":"2026-10-30T08:00:00Z","comments":2,
            "url":"https://github.com/acme/widget/issues/12"}]"#;
        let issues = parse_open_issues(gh).expect("issues");
        assert_eq!(
            issues,
            [OpenIssue {
                number: 12,
                title: "Crash on start".into(),
                labels: vec!["bug".into()],
                author: "sam".into(),
                opened_at: DateTime::parse_from_rfc3339("2026-10-30T08:00:00Z")
                    .expect("a time")
                    .with_timezone(&Utc),
                comments: 2,
                url: "https://github.com/acme/widget/issues/12".into(),
            }]
        );
        let tea = r#"[
            {"number":4,"title":"Slow","labels":[{"name":"perf"}],"user":{"login":"kim"},
             "created_at":"2026-10-29T10:00:00+02:00","comments":0,
             "url":"https://git.example.com/api/v1/repos/acme/widget/issues/4",
             "html_url":"https://git.example.com/acme/widget/issues/4","pull_request":null},
            {"number":5,"title":"A PR","labels":[],"user":{"login":"kim"},
             "created_at":"2026-10-29T10:00:00Z","comments":0,
             "html_url":"https://git.example.com/acme/widget/pulls/5",
             "pull_request":{"merged":false}}
        ]"#;
        let issues = parse_open_issues(tea).expect("issues");
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].author, "kim");
        assert_eq!(issues[0].labels, ["perf"]);
        assert_eq!(
            issues[0].url,
            "https://git.example.com/acme/widget/issues/4"
        );
        assert!(parse_open_issues("gh: not logged in").is_err());
    }

    #[test]
    fn whoami_and_comment_render_and_parse() {
        assert_eq!(
            HostCommand::WhoAmI.render(&github(), &repo()),
            "gh api user --jq .login"
        );
        assert_eq!(
            HostCommand::WhoAmI.render(&forgejo(), &repo()),
            "teacli whoami --host git.example.com --format json"
        );
        let comment = HostCommand::CommentIssue {
            number: 4,
            body: "Fixed in #5",
        };
        assert_eq!(
            comment.render(&github(), &repo()),
            "gh issue comment 4 -R acme/widget --body 'Fixed in #5'"
        );
        assert_eq!(
            comment.render(&forgejo(), &repo()),
            "teacli issue comment create acme/widget 4 --host git.example.com -b 'Fixed in #5' \
             --format json"
        );
        assert_eq!(parse_login("sam\n").expect("a login"), "sam");
        assert_eq!(
            parse_login(r#"{"id": 3, "login": "kim"}"#).expect("a login"),
            "kim"
        );
        assert!(parse_login("").is_err());
        assert!(parse_login("not logged in").is_err());
    }

    #[test]
    fn a_viewed_issue_parses_and_a_pr_is_refused() {
        let gh = r#"{"number":12,"title":"Crash","body":"Steps","state":"OPEN",
            "author":{"login":"sam"},"url":"https://github.com/acme/widget/issues/12"}"#;
        let issue = parse_viewed_issue(gh).expect("an issue");
        assert!(issue.open);
        assert_eq!(issue.author, "sam");
        assert_eq!(issue.body, "Steps");
        let pr = r#"{"number":5,"title":"PR","body":"","state":"open","pull_request":{}}"#;
        assert!(parse_viewed_issue(pr).is_err());
    }

    #[test]
    fn gh_and_teacli_render() {
        let create = HostCommand::CreateIssue {
            title: "Fix it",
            body: "More",
        };
        assert_eq!(
            create.render(&github(), &repo()),
            "GH_TOKEN=\"${BEEP_GH_TOKEN_ACME:-$GH_TOKEN}\" gh issue create -R acme/widget --title 'Fix it' --body More --label beep"
        );
        assert_eq!(
            create.render(&forgejo(), &repo()),
            "teacli issue create acme/widget --host git.example.com -t 'Fix it' -b More \
             --label beep --format json"
        );
        let close = HostCommand::CloseIssue {
            number: 4,
            comment: "Done in #5",
            not_planned: true,
        };
        assert_eq!(
            close.render(&github(), &repo()),
            "GH_TOKEN=\"${BEEP_GH_TOKEN_ACME:-$GH_TOKEN}\" gh issue close 4 -R acme/widget --comment 'Done in #5' --reason 'not planned'"
        );
        assert_eq!(
            close.render(&forgejo(), &repo()),
            "teacli issue comment create acme/widget 4 --host git.example.com -b 'Done in #5' \
             --format json && teacli issue close acme/widget 4 --host git.example.com --format json"
        );
        let relabel = HostCommand::Relabel {
            number: 4,
            add: Some(LABEL_REVIEW),
            remove: &[LABEL_RUNNING, LABEL_BLOCKED],
        };
        assert_eq!(
            relabel.render(&github(), &repo()),
            "GH_TOKEN=\"${BEEP_GH_TOKEN_ACME:-$GH_TOKEN}\" gh issue edit 4 -R acme/widget --add-label beep:review \
             --remove-label beep:running --remove-label beep:blocked"
        );
        assert_eq!(
            relabel.render(&forgejo(), &repo()),
            "teacli issue label add acme/widget 4 beep:review --host git.example.com --format json \
             && teacli issue label remove acme/widget 4 beep:running beep:blocked \
             --host git.example.com --format json"
        );
        let find = HostCommand::FindPr {
            branch: "beep/fix-it",
            base: "main",
        };
        assert_eq!(
            find.render(&forgejo(), &repo()),
            "teacli pr find acme/widget main beep/fix-it --host git.example.com --format json"
        );
        assert!(find.render(&github(), &repo()).starts_with(
            "GH_TOKEN=\"${BEEP_GH_TOKEN_ACME:-$GH_TOKEN}\" gh pr list -R acme/widget --head"
        ));
        assert_eq!(
            HostCommand::MergePr { number: 7 }.render(&github(), &repo()),
            "GH_TOKEN=\"${BEEP_GH_TOKEN_ACME:-$GH_TOKEN}\" gh pr merge 7 -R acme/widget --squash"
        );
        assert_eq!(
            HostCommand::Visibility.render(&github(), &repo()),
            "GH_TOKEN=\"${BEEP_GH_TOKEN_ACME:-$GH_TOKEN}\" gh repo view acme/widget --json visibility"
        );
        assert_eq!(
            HostCommand::Visibility.render(&forgejo(), &repo()),
            "teacli repo get acme/widget --host git.example.com --format json"
        );
        let list = HostCommand::ListOwnedIssues.render(&github(), &repo());
        assert!(
            list.contains("--label beep --state all --limit 50"),
            "{list}"
        );
        assert!(
            HostCommand::EnsureLabels
                .render(&github(), &repo())
                .contains("--force")
        );
        assert!(
            HostCommand::EnsureLabels
                .render(&forgejo(), &repo())
                .contains("label create")
        );
    }

    #[test]
    fn gh_runs_as_the_account_for_the_owner() {
        let repo = RepoName::parse("octo-two/tool").unwrap();
        assert_eq!(
            HostCommand::MergePr { number: 1 }.render(&github(), &repo),
            "GH_TOKEN=\"${BEEP_GH_TOKEN_OCTO_TWO:-$GH_TOKEN}\" gh pr merge 1 -R octo-two/tool --squash"
        );
        // A name GitHub would not give an owner gets no account var.
        let odd = RepoName::parse("my.org/tool").unwrap();
        assert_eq!(
            HostCommand::MergePr { number: 1 }.render(&github(), &odd),
            "gh pr merge 1 -R my.org/tool --squash"
        );
        // Forgejo is untouched.
        assert!(
            HostCommand::MergePr { number: 1 }
                .render(&forgejo(), &repo)
                .starts_with("teacli ")
        );
    }

    #[test]
    fn text_for_a_host_is_checked() {
        let public_text = PublicText::new(&["build-box".to_owned()], &[]);
        let create = HostCommand::CreateIssue {
            title: "Fix",
            body: "Failed on build-box",
        };
        assert_eq!(create.leaks(&public_text), ["build-box"]);
        assert!(
            HostCommand::ReopenIssue { number: 1 }
                .leaks(&public_text)
                .is_empty()
        );
    }

    #[test]
    fn created_issue_numbers_parse() {
        let created =
            parse_created_issue(&github(), "\nhttps://github.com/acme/widget/issues/12\n")
                .expect("an issue");
        assert_eq!(created.number, 12);
        assert_eq!(created.url, "https://github.com/acme/widget/issues/12");
        let created = parse_created_issue(
            &forgejo(),
            r#"{"number": 7, "html_url": "https://git.example.com/acme/widget/issues/7"}"#,
        )
        .expect("an issue");
        assert_eq!(created.number, 7);
        assert_eq!(created.url, "https://git.example.com/acme/widget/issues/7");
        assert!(parse_created_issue(&github(), "no issue was made").is_err());
    }

    #[test]
    fn owned_issues_parse() {
        let gh = r#"[{"number":3,"state":"CLOSED","title":"A","comments":[{"id":1},{"id":2}]}]"#;
        let issues = parse_owned_issues(&github(), gh).expect("issues");
        assert_eq!(
            issues,
            [OwnedIssue {
                number: 3,
                open: false,
                title: "A".into(),
                comments: 2
            }]
        );
        let tea = r#"[{"number":4,"state":"open","title":"B","comments":5}]"#;
        let issues = parse_owned_issues(&forgejo(), tea).expect("issues");
        assert_eq!(
            issues,
            [OwnedIssue {
                number: 4,
                open: true,
                title: "B".into(),
                comments: 5
            }]
        );
    }

    fn gh_pr(rollup: &str, review: &str, merged_at: &str) -> String {
        format!(
            r#"{{"number":9,"state":"OPEN","mergedAt":{merged_at},"reviewDecision":"{review}",
            "statusCheckRollup":{rollup},"url":"https://github.com/acme/widget/pull/9",
            "title":"Fix","body":"Body"}}"#
        )
    }

    #[test]
    fn pr_state_parses_checks() {
        let pr = |rollup: &str| {
            parse_pr(&github(), &gh_pr(rollup, "APPROVED", "null"))
                .expect("json")
                .expect("a pr")
        };
        let failing = pr(r#"[{"conclusion":"SUCCESS","status":"COMPLETED"},
                            {"conclusion":"FAILURE","status":"COMPLETED"},
                            {"conclusion":"","status":"IN_PROGRESS"}]"#);
        assert_eq!(failing.checks, ChecksState::Failing);
        let passing = pr(r#"[{"conclusion":"SUCCESS","status":"COMPLETED"},
                            {"conclusion":"SKIPPED","status":"COMPLETED"}]"#);
        assert_eq!(passing.checks, ChecksState::Passing);
        assert_eq!(
            pr(r#"[{"conclusion":"SUCCESS"},{"conclusion":"","status":"QUEUED"}]"#).checks,
            ChecksState::Pending
        );
        assert_eq!(pr("[]").checks, ChecksState::None);
        assert_eq!(passing.review.as_deref(), Some("APPROVED"));
        assert!(passing.open && !passing.merged);
        assert_eq!(passing.number, 9);
        assert_eq!(passing.title, "Fix");
    }

    #[test]
    fn pr_state_reads_merges_lists_and_forgejo() {
        let merged = parse_pr(&github(), &gh_pr("[]", "", r#""2026-01-02T03:04:05Z""#))
            .expect("json")
            .expect("a pr");
        assert!(merged.merged);
        assert_eq!(merged.review, None);
        assert_eq!(parse_pr(&github(), "[]").expect("json"), None);
        assert_eq!(parse_pr(&github(), "").expect("empty"), None);
        let listed = format!("[{}]", gh_pr("[]", "", "null"));
        assert_eq!(
            parse_pr(&github(), &listed)
                .expect("json")
                .map(|pr| pr.number),
            Some(9)
        );
        let tea = r#"{"number":2,"state":"closed","merged":true,
            "html_url":"https://git.example.com/acme/widget/pulls/2","title":"T","body":"B"}"#;
        let pr = parse_pr(&forgejo(), tea).expect("json").expect("a pr");
        assert!(pr.merged && !pr.open);
        assert_eq!(pr.checks, ChecksState::None);
        assert_eq!(pr.url, "https://git.example.com/acme/widget/pulls/2");
        assert!(parse_pr(&forgejo(), r#"{"error":true,"message":"not found"}"#).is_err());
    }

    #[test]
    fn fingerprints_change_with_the_state() {
        let pr = parse_pr(
            &github(),
            &gh_pr(r#"[{"conclusion":"SUCCESS"}]"#, "APPROVED", "null"),
        )
        .expect("json")
        .expect("a pr");
        assert_eq!(pr_fingerprint(&pr), "open|false|APPROVED|passing");
        let failing = PrState {
            checks: ChecksState::Failing,
            ..pr.clone()
        };
        assert_ne!(pr_fingerprint(&pr), pr_fingerprint(&failing));
    }

    #[test]
    fn unreadable_visibility_is_public() {
        assert_eq!(
            parse_visibility(&github(), r#"{"visibility":"PRIVATE"}"#),
            Visibility::Private
        );
        assert_eq!(
            parse_visibility(&github(), r#"{"visibility":"PUBLIC"}"#),
            Visibility::Public
        );
        assert_eq!(
            parse_visibility(&forgejo(), r#"{"private":true}"#),
            Visibility::Private
        );
        assert_eq!(
            parse_visibility(&forgejo(), r#"{"private":false}"#),
            Visibility::Public
        );
        for out in ["", "not json", "{}", "[]"] {
            assert_eq!(
                parse_visibility(&github(), out),
                Visibility::Public,
                "{out}"
            );
        }
    }

    #[test]
    fn output_is_capped_on_a_char_boundary() {
        let mut out = "a".repeat(MAX_OUTPUT - 1);
        push_capped(&mut out, "éé");
        assert_eq!(out.len(), MAX_OUTPUT - 1);
        push_capped(&mut out, "b");
        push_capped(&mut out, "c");
        assert_eq!(out.len(), MAX_OUTPUT);
    }

    #[test]
    fn found_issues_must_hold_the_marker() {
        let gh = HostCommand::FindIssue {
            marker: "beep-task:1",
        }
        .render(&github(), &repo());
        assert!(gh.contains("--jq"), "{gh}");
        assert!(gh.contains(r#"contains("beep-task:1")"#), "{gh}");
        let tea = HostCommand::FindIssue {
            marker: "beep-task:1",
        }
        .render(&forgejo(), &repo());
        assert!(tea.contains("-q beep-task:1"), "{tea}");
        let out = r#"[{"number": 4, "html_url": "https://git.example.com/acme/widget/issues/4", "body": "x beep-task:2"},
                      {"number": 5, "html_url": "https://git.example.com/acme/widget/issues/5", "body": "y <!-- beep-task:1 -->"}]"#;
        let found = parse_found_issue(out, "beep-task:1").unwrap();
        assert_eq!(found.map(|issue| issue.number), Some(5));
        assert_eq!(parse_found_issue("", "beep-task:1").unwrap(), None);
        assert_eq!(parse_found_issue("[]", "beep-task:1").unwrap(), None);
    }
}
