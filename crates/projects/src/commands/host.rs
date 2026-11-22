//! `gh` and `tea` command lines for one host operation.

use super::gh_account;
use crate::{
    model::{IssueHost, RepoName},
    quote,
};

const GH_PR_FIELDS: &str = "number,title,body,url,author,state,isDraft,labels,createdAt,comments,reviews,reviewDecision,statusCheckRollup";

const GH_ISSUE_FIELDS: &str = "number,title,body,url,author,state,labels,createdAt,comments";

/// What Beep asks a code host for.
#[derive(Debug, Clone, Copy)]
pub enum HostCommand<'a> {
    /// The newest open issues, any author's.
    ListOpenIssues,
    /// The numbers of the open issues a pull request is linked to, to close them. GitHub only.
    ListLinkedIssues,
    /// The newest open pull requests, any author's.
    ListOpenPrs,
    /// The branch one pull request is made from, and whether it is a fork's.
    ViewPrHead {
        number: u64,
    },
    /// One pull request or issue in full: its text, state, comments and, for a pull request,
    /// its reviews and checks.
    ViewThread {
        number: u64,
    },
    CommentIssue {
        number: u64,
        body: &'a str,
    },
    /// A comment on a pull request's conversation.
    CommentPr {
        number: u64,
        body: &'a str,
    },
    /// The pull requests that will close issue `number` or mention it. GitHub only.
    LinkedPrs {
        number: u64,
    },
    /// The login of the account the CLI is signed in as.
    WhoAmI,
    /// The repository as the CLI's account sees it, with what that account may do there.
    CanPush,
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
    /// Whether this only reads, so two at once may share one run.
    pub fn only_reads(&self) -> bool {
        match self {
            Self::ListOpenIssues
            | Self::ListLinkedIssues
            | Self::ListOpenPrs
            | Self::ViewPrHead { .. }
            | Self::ViewThread { .. }
            | Self::LinkedPrs { .. }
            | Self::WhoAmI
            | Self::CanPush => true,
            Self::CommentIssue { .. } | Self::CommentPr { .. } => false,
        }
    }

    /// The shell command that runs this against `repo` on `host`; `None` when the host has no
    /// way to answer it.
    pub fn render(&self, host: &IssueHost, repo: &RepoName) -> Option<String> {
        match host {
            IssueHost::Github => Some(self.gh(&repo.to_string())),
            IssueHost::Forgejo(host) => self.teacli(host.as_ref(), &repo.to_string()),
        }
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
            Self::ListOpenIssues => {
                let limit = GH_OPEN_LIMIT.to_string();
                let flags = [&["--state", "open", "--limit", &limit][..], &GH_ISSUES].concat();
                gh(&["issue", "list"], &flags)
            }
            Self::ListLinkedIssues => {
                let limit = GH_OPEN_LIMIT.to_string();
                gh(
                    &["issue", "list"],
                    &[
                        "--state",
                        "open",
                        "--search",
                        "linked:pr",
                        "--limit",
                        &limit,
                        "--json",
                        "number",
                        "--jq",
                        "[.[].number]",
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
            Self::ViewPrHead { number } => number_command(
                &["pr", "view"],
                number,
                &["--json", "number,headRefName,isCrossRepository"],
            ),
            // A number that is not a pull request is an issue.
            Self::ViewThread { number } => format!(
                "{} 2>/dev/null || {}",
                number_command(&["pr", "view"], number, &["--json", GH_PR_FIELDS]),
                number_command(&["issue", "view"], number, &["--json", GH_ISSUE_FIELDS]),
            ),
            Self::CommentIssue { number, body } => {
                number_command(&["issue", "comment"], number, &["--body", body])
            }
            Self::CommentPr { number, body } => {
                number_command(&["pr", "comment"], number, &["--body", body])
            }
            // `gh api` takes no -R; the owner's account decides who it is.
            Self::LinkedPrs { number } => {
                let (owner, name) = repo.split_once('/').unwrap_or((repo, ""));
                format!(
                    "{account}{}",
                    Parts::new(&["gh", "api", "graphql"])
                        .with(&["-f", &format!("owner={owner}")])
                        .with(&["-f", &format!("name={name}")])
                        .with(&["-F", &format!("number={number}")])
                        .with(&["-f", &format!("query={LINKED_PRS_QUERY}")])
                        .with(&["--jq", LINKED_PRS_JQ])
                        .render()
                )
            }
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
        }
    }

    pub(super) fn teacli(&self, host: &str, repo: &str) -> Option<String> {
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

        Some(match *self {
            Self::ListOpenIssues => tea(&["issue", "list"], &[], &["-s", "open", "--all"]),
            // Forgejo cannot search issues by their linked pull requests.
            Self::ListLinkedIssues | Self::LinkedPrs { .. } => return None,
            Self::ListOpenPrs => tea(&["pr", "list"], &[], &["-s", "open", "--all"]),
            Self::ViewPrHead { number } => tea(&["pr", "get"], &[&number.to_string()], &[]),
            // Forgejo keeps a pull request's conversation as its issue's comments.
            Self::ViewThread { number } => {
                let number = number.to_string();
                format!(
                    "{} && {}",
                    tea(&["issue", "get"], &[&number], &[]),
                    tea(&["issue", "comment", "list"], &[&number], &["--all"]),
                )
            }
            // Forgejo keeps a pull request's conversation as its issue's comments.
            Self::CommentIssue { number, body } | Self::CommentPr { number, body } => tea(
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
        })
    }
}

/// An issue's pull requests: those that will close it, and those whose text mentions it.
const LINKED_PRS_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { \
    repository(owner: $owner, name: $name) { issue(number: $number) { \
    closedByPullRequestsReferences(first: 20, includeClosedPrs: true) { nodes { number title url state } } \
    timelineItems(itemTypes: [CROSS_REFERENCED_EVENT], first: 50) { nodes { ... on CrossReferencedEvent { \
    source { ... on PullRequest { number title url state } } } } } } } }";

/// Both lists of [`LINKED_PRS_QUERY`] as one, each pull request once.
const LINKED_PRS_JQ: &str = ".data.repository.issue | [.closedByPullRequestsReferences.nodes[], \
    (.timelineItems.nodes[].source | select(.number != null))] | unique_by(.url)";

/// Most open issues or pull requests one gh list reads; gh fetches the pages itself.
pub(crate) const GH_OPEN_LIMIT: usize = 1000;

/// What `gh issue list` keeps of each issue: only what the list shows, so a long list stays
/// small.
pub(crate) const GH_ISSUES: [&str; 4] = [
    "--json",
    "number,title,labels,author,createdAt,comments,url,assignees",
    "--jq",
    "[.[] | {number, title, labels: [.labels[].name], author: .author.login, createdAt, \
     comments: (.comments | length), url, assignees: [.assignees[].login]}]",
];
