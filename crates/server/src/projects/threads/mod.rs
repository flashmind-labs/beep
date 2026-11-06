//! The user's open PRs and issues on a project's host: replies due, reading, replying, chats.

mod chat;
mod parse;
use crate::projects::stored::StoredProject;
use beep_projects::issues::SourceError;
use beep_projects::issues::issue_source;
use chat::card;
pub(crate) use chat::{ask_for_reply, chat_context, open_chat};
pub(crate) use parse::{ListedThread, Thread, needs_reply, parse_thread};
use parse::{is_me, last_other, parse_threads};

use super::{fleet::Fleet, issues::IssueError};
use crate::{
    account::user::User,
    protocol::{GitItemKind, ThreadView},
    store::ProjectRow,
};
use anyhow::Result;
use beep_projects::{
    Shell,
    commands::{HostCommand, gh_account, parse_login},
    issues::{clipped, run_host, run_rendered},
    model::{IssueHost, RepoName},
    quote,
};
use std::sync::Arc;
use strum::{EnumString, IntoStaticStr};

/// The most of a thread's text one read gives the agent.
const MAX_THREAD_TEXT: usize = 24 * 1024;
/// The most comments one read gives, newest kept.
const MAX_COMMENTS: usize = 30;
/// The most comments a thread card shows, newest kept.
const CARD_COMMENTS: usize = 20;
/// The most of one post a thread card shows.
const CARD_TEXT: usize = 2000;
/// The most of the last comment `last_other` shows.
const LAST_LINE: usize = 120;
/// The most threads one list reads of each kind.
const LIST_LIMIT: &str = "50";

/// Whether a thread is a pull request or an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr, EnumString)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ThreadKind {
    PullRequest,
    Issue,
}

impl From<ThreadKind> for GitItemKind {
    fn from(kind: ThreadKind) -> Self {
        match kind {
            ThreadKind::PullRequest => GitItemKind::PullRequest {},
            ThreadKind::Issue => GitItemKind::Issue {},
        }
    }
}

impl ThreadKind {
    /// "PR #41" or "issue #31".
    pub(crate) fn label(self, number: u64) -> String {
        match self {
            ThreadKind::PullRequest => format!("PR #{number}"),
            ThreadKind::Issue => format!("issue #{number}"),
        }
    }

    fn gh(self) -> &'static str {
        match self {
            ThreadKind::PullRequest => "pr",
            ThreadKind::Issue => "issue",
        }
    }
}

// ---- gh ----

/// The `gh` commands this module runs. Forgejo has none yet.
enum ThreadCommand<'a> {
    /// The open threads of `kind` the signed-in account opened.
    ListMine(ThreadKind),
    /// One thread with its comments, a pull request or else an issue.
    View { number: u64 },
    Comment {
        kind: ThreadKind,
        number: u64,
        body: &'a str,
    },
}

impl ThreadCommand<'_> {
    fn render(&self, repo: &str) -> String {
        let gh = |words: &[&str]| {
            let mut parts = vec!["gh".to_owned()];
            parts.extend(words.iter().map(|word| quote(word)));
            parts.extend(["-R".to_owned(), quote(repo)]);
            format!("{}{}", gh_account(repo), parts.join(" "))
        };
        match *self {
            Self::ListMine(kind) => {
                let fields = match kind {
                    ThreadKind::PullRequest => "number,title,url,author,updatedAt,comments,reviews",
                    ThreadKind::Issue => "number,title,url,author,updatedAt,comments",
                };
                gh(&[
                    kind.gh(),
                    "list",
                    "--author",
                    "@me",
                    "--state",
                    "open",
                    "--limit",
                    LIST_LIMIT,
                    "--json",
                    fields,
                ])
            }
            Self::View { number } => {
                let number = number.to_string();
                format!(
                    "{} 2>/dev/null || {}",
                    gh(&[
                        "pr",
                        "view",
                        &number,
                        "--json",
                        "number,title,body,url,author,updatedAt,comments,reviews",
                    ]),
                    gh(&[
                        "issue",
                        "view",
                        &number,
                        "--json",
                        "number,title,body,url,author,updatedAt,comments",
                    ]),
                )
            }
            Self::Comment { kind, number, body } => {
                gh(&[kind.gh(), "comment", &number.to_string(), "--body", body])
            }
        }
    }
}

// ---- reading ----

fn host_failed(error: &anyhow::Error) -> IssueError {
    IssueError::from(SourceError::Refused(format!(
        "Could not read the host: {error:#}"
    )))
}

/// A shell on a machine with the project; `None` when its host is not GitHub.
async fn github_shell(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Option<Arc<dyn Shell>>, IssueError> {
    Ok(issue_source(&StoredProject::new(user, project), fleet)
        .await?
        .filter(|source| source.host == IssueHost::Github)
        .map(|source| source.shell))
}

/// The login `gh` is signed in as, or empty when unknown.
async fn whoami(shell: &dyn Shell, project: &ProjectRow) -> String {
    let login = run_host(
        shell,
        &IssueHost::Github,
        &project.name,
        HostCommand::WhoAmI,
    )
    .await
    .and_then(|out| parse_login(&out));
    login.unwrap_or_else(|error| {
        log::info!("projects: the host's account is unknown: {error:#}");
        String::new()
    })
}

/// The user's open PRs and issues on the project's GitHub repo, newest activity first.
pub(crate) async fn list(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Vec<ListedThread>, IssueError> {
    let Some(shell) = github_shell(fleet, user, project).await? else {
        return Ok(Vec::new());
    };
    let repo = project.name.to_string();
    let me = whoami(shell.as_ref(), project).await;
    let mut out = Vec::new();
    for kind in [ThreadKind::PullRequest, ThreadKind::Issue] {
        let printed = run_rendered(shell.as_ref(), &ThreadCommand::ListMine(kind).render(&repo))
            .await
            .map_err(|error| host_failed(&error))?;
        let threads = parse_threads(&printed).map_err(|error| host_failed(&error))?;
        out.extend(threads.into_iter().map(|thread| {
            ListedThread {
                kind,
                repo: repo.clone(),
                number: thread.number,
                last_other: last_other(&thread.comments, &me),
                needs_reply: needs_reply(&thread.comments, &me),
                updated_at: thread
                    .updated_at
                    .or_else(|| thread.comments.last().map(|c| c.at))
                    .unwrap_or_default(),
                title: thread.title,
                url: thread.url,
            }
        }));
    }
    out.sort_by(|a, b| {
        b.needs_reply
            .cmp(&a.needs_reply)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    Ok(out)
}

/// `repo` checked to be `project`'s, as it is kept: lowercase.
pub(crate) fn project_repo(project: &ProjectRow, repo: &str) -> Result<String, IssueError> {
    let name = project.name.to_string().to_ascii_lowercase();
    let asked = RepoName::parse(repo.trim())
        .map_err(|_| {
            IssueError::from(SourceError::Refused(format!(
                "{repo} is not an org/repo name."
            )))
        })?
        .to_string()
        .to_ascii_lowercase();
    if asked != name {
        return Err(IssueError::from(SourceError::Refused(format!(
            "{repo} is not this project's repository; Beep reads and replies on {name} only."
        ))));
    }
    Ok(name)
}

/// A thread read now, with who `gh` is signed in as.
pub(crate) struct ReadThread {
    pub(crate) thread: Thread,
    /// Empty when unknown.
    pub(crate) me: String,
    shell: Arc<dyn Shell>,
    repo: String,
}

impl ReadThread {
    /// Whether someone else opened it; an unknown user counts as someone else.
    pub(crate) fn foreign(&self) -> bool {
        !is_me(&self.thread.author, &self.me)
    }

    pub(crate) fn last_other(&self) -> String {
        last_other(&self.thread.comments, &self.me)
    }

    pub(crate) async fn comment(&self, body: &str) -> Result<()> {
        let command = ThreadCommand::Comment {
            kind: self.thread.kind,
            number: self.thread.number,
            body,
        };
        run_rendered(self.shell.as_ref(), &command.render(&self.repo)).await?;
        Ok(())
    }
}

/// Reads thread `number` of the project's GitHub repo again with one `gh` call.
pub(crate) async fn read(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    number: u64,
) -> Result<ReadThread, IssueError> {
    let Some(shell) = github_shell(fleet, user, project).await? else {
        return Err(IssueError::from(SourceError::Refused(
            "Beep reads pull requests and issues on GitHub only for now.".to_owned(),
        )));
    };
    let repo = project.name.to_string();
    let printed = run_rendered(
        shell.as_ref(),
        &ThreadCommand::View { number }.render(&repo),
    )
    .await
    .map_err(|error| host_failed(&error))?;
    let thread = parse_thread(&printed).map_err(|error| host_failed(&error))?;
    if thread.number != number {
        return Err(IssueError::from(SourceError::Refused(format!(
            "The host gave #{} for #{number}.",
            thread.number
        ))));
    }
    let me = whoami(shell.as_ref(), project).await;
    Ok(ReadThread {
        thread,
        me,
        shell,
        repo,
    })
}

/// Thread `number` of `repo` as its chat card shows it, long text cut short.
pub(crate) async fn view(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    repo: &str,
    number: u64,
) -> Result<ThreadView, IssueError> {
    project_repo(project, repo)?;
    let read = read(fleet, user, project, number).await?;
    Ok(card(&read.thread, &read.me))
}

#[cfg(test)]
pub(crate) mod tests;
