//! `gh` and `tea` command lines for one host operation.

use super::gh_account;
use crate::{
    leak::PublicText,
    model::{IssueHost, RepoName},
    quote,
};
use serde_json::{Value, json};

/// The label every issue Beep files carries.
pub const LABEL_OWNED: &str = "beep";

pub const LABEL_RUNNING: &str = "beep:running";

pub const LABEL_BLOCKED: &str = "beep:blocked";

pub const LABEL_REVIEW: &str = "beep:review";

/// The labels [`HostCommand::EnsureLabels`] creates, with their colours.
pub(super) const LABELS: [(&str, &str); 4] = [
    (LABEL_OWNED, "5319e7"),
    (LABEL_RUNNING, "0e8a16"),
    (LABEL_BLOCKED, "d93f0b"),
    (LABEL_REVIEW, "1d76db"),
];

pub(super) const LABEL_DESCRIPTION: &str = "Managed by the project agent";

/// What Beep asks a code host for.
#[derive(Debug, Clone, Copy)]
pub enum HostCommand<'a> {
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
    /// The newest open pull requests, any author's.
    ListOpenPrs,
    /// One issue with its text and author.
    ViewIssue {
        number: u64,
    },
    /// The branch one pull request is made from, and whether it is a fork's.
    ViewPrHead {
        number: u64,
    },
    CommentIssue {
        number: u64,
        body: &'a str,
    },
    /// The login of the account the CLI is signed in as.
    WhoAmI,
    /// The repository as the CLI's account sees it, with what that account may do there.
    CanPush,
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
    /// One review that comments, with an inline comment per entry.
    PostReview {
        number: u64,
        commit: &'a str,
        body: &'a str,
        comments: &'a [InlineComment],
    },
    Visibility,
}

/// A comment on one line of a pull request's diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineComment {
    pub path: String,
    pub line: u32,
    pub body: String,
}

/// The pieces of a command, quoted when joined.
pub(super) struct Parts(Vec<String>);

impl Parts {
    pub(super) fn new(words: &[&str]) -> Parts {
        Parts(words.iter().map(|word| quote(word)).collect())
    }

    pub(super) fn with(mut self, words: &[&str]) -> Parts {
        self.0.extend(words.iter().map(|word| quote(word)));
        self
    }

    pub(super) fn render(self) -> String {
        self.0.join(" ")
    }
}

impl HostCommand<'_> {
    /// The shell command that runs this against `repo` on `host`.
    pub fn render(&self, host: &IssueHost, repo: &RepoName) -> String {
        match host {
            IssueHost::Github => self.gh(&repo.to_string()),
            IssueHost::Forgejo(host) => self.teacli(host.as_str(), &repo.to_string()),
        }
    }

    /// The text this command sends to the host, for [`Self::leaks`].
    pub(super) fn outgoing(&self) -> Vec<&str> {
        match self {
            Self::CreateIssue { title, body } | Self::EditIssue { title, body, .. } => {
                vec![title, body]
            }
            Self::CloseIssue { comment, .. } => vec![comment],
            Self::CommentIssue { body, .. } => vec![body],
            Self::PostReview { body, comments, .. } => {
                let mut out = vec![*body];
                out.extend(comments.iter().map(|comment| comment.body.as_str()));
                out
            }
            _ => Vec::new(),
        }
    }

    /// The private details in the text this command would publish, each once.
    pub fn leaks(&self, public_text: &PublicText) -> Vec<String> {
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

    pub(super) fn gh(&self, repo: &str) -> String {
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
            Self::ListOpenPrs => {
                let jq = "[.[] | {number, title, url, author: .author.login, \
                          isDraft, createdAt, updatedAt, headRefName, \
                          labels: [.labels[].name]}]";
                let limit = GH_OPEN_LIMIT.to_string();
                gh(
                    &["pr", "list"],
                    &[
                        "--state",
                        "open",
                        "--limit",
                        &limit,
                        "--json",
                        "number,title,url,author,isDraft,createdAt,updatedAt,headRefName,labels",
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
            Self::ViewPrHead { number } => number_command(
                &["pr", "view"],
                number,
                &["--json", "number,headRefName,isCrossRepository"],
            ),
            Self::CommentIssue { number, body } => {
                number_command(&["issue", "comment"], number, &["--body", body])
            }
            // `gh api` takes no -R; the owner's account decides who it is.
            Self::WhoAmI => format!(
                "{account}{}",
                Parts::new(&["gh", "api", "user", "--jq", ".login"]).render()
            ),
            Self::CanPush => format!(
                "{account}{}",
                Parts::new(&[
                    "gh",
                    "api",
                    &format!("repos/{repo}"),
                    "--jq",
                    ".permissions.push"
                ])
                .render()
            ),
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
            Self::PostReview {
                number,
                commit,
                body,
                comments,
            } => {
                let payload = json!({
                    "commit_id": commit,
                    "event": "COMMENT",
                    "body": body,
                    "comments": comments
                        .iter()
                        .map(|comment| json!({
                            "path": comment.path,
                            "line": comment.line,
                            "side": "RIGHT",
                            "body": comment.body,
                        }))
                        .collect::<Vec<_>>(),
                });
                // `gh api` takes no -R; the owner's account decides who it is.
                format!(
                    "printf '%s' {} | {account}{}",
                    quote(&payload.to_string()),
                    Parts::new(&["gh", "api", &format!("repos/{repo}/pulls/{number}/reviews")])
                        .with(&["--method", "POST", "--input", "-"])
                        .render()
                )
            }
            Self::Visibility => format!(
                "{account}{}",
                Parts::new(&["gh", "repo", "view", repo])
                    .with(&["--json", "visibility"])
                    .render()
            ),
        }
    }

    pub(super) fn teacli(&self, host: &str, repo: &str) -> String {
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
            Self::ListOpenIssues => tea(&["issue", "list"], &[], &["-s", "open", "--all"]),
            Self::ListOpenPrs => tea(&["pr", "list"], &[], &["-s", "open", "--all"]),
            Self::ViewIssue { number } => tea(&["issue", "get"], &[&number.to_string()], &[]),
            Self::ViewPrHead { number } => tea(&["pr", "get"], &[&number.to_string()], &[]),
            Self::CommentIssue { number, body } => tea(
                &["issue", "comment", "create"],
                &[&number.to_string()],
                &["-b", body],
            ),
            Self::WhoAmI => Parts::new(&["teacli", "whoami", "--host", host])
                .with(&["--format", "json"])
                .render(),
            Self::CanPush => Parts::new(&["teacli", "api", &format!("repos/{repo}")])
                .with(&["--host", host])
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
            Self::PostReview {
                number,
                commit,
                body,
                comments,
            } => {
                let inline: Vec<String> = comments
                    .iter()
                    .map(|comment| format!("{}:{}:{}", comment.path, comment.line, comment.body))
                    .collect();
                let mut flags = vec!["--state", "comment", "-b", body, "--commit", commit];
                for comment in &inline {
                    flags.extend(["--comment", comment.as_str()]);
                }
                tea(&["pr", "review", "submit"], &[&number.to_string()], &flags)
            }
            Self::Visibility => tea(&["repo", "get"], &[], &[]),
        }
    }
}

/// Most open issues or pull requests one gh list reads; gh fetches the pages itself.
const GH_OPEN_LIMIT: usize = 1000;

pub(super) const PR_FIELDS: &str =
    "state,mergedAt,reviewDecision,statusCheckRollup,url,number,title,body";
