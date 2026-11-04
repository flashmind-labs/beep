//! Listing open issues and pull requests.

use super::{
    super::{
        commands::{
            HostCommand, OpenPr, PrHead, parse_login, parse_open_issues, parse_open_prs,
            parse_pr_head,
        },
        fleet::Fleet,
        model::IssueHost,
    },
    IssueError, OpenIssue, issue_sources, read_any, run_host,
};
use crate::{account::user::User, store::ProjectRow};
use anyhow::Result;

/// The issues of `open` that no task tracks, newest first.
pub(crate) fn untracked(open: Vec<OpenIssue>, tracked: &[u64]) -> Vec<OpenIssue> {
    let mut left: Vec<OpenIssue> = open
        .into_iter()
        .filter(|issue| !tracked.contains(&issue.number))
        .collect();
    left.sort_by(|a, b| {
        b.opened_at
            .cmp(&a.opened_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    left
}

/// Open issues on `project`'s host no task tracks, newest first; empty without a host.
#[cfg(test)]
pub(crate) async fn list_open(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Vec<OpenIssue>, IssueError> {
    match read_open(fleet, user, project).await? {
        Some(read) => untracked_now(user, project, &read).await,
        None => Ok(Vec::new()),
    }
}

/// Every open issue on a project's host, as it was read.
#[derive(Debug, Clone)]
pub(crate) struct HostIssues {
    pub(crate) host: IssueHost,
    pub(crate) open: Vec<OpenIssue>,
}

/// Every open issue on `project`'s host, tracked or not; `None` without a host.
pub(crate) async fn read_open(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Option<HostIssues>, IssueError> {
    let Some(sources) = issue_sources(fleet, user, project).await? else {
        return Ok(None);
    };
    let (source, open) = read_any(
        fleet,
        &sources,
        project,
        HostCommand::ListOpenIssues,
        parse_open_issues,
    )
    .await?;
    Ok(Some(HostIssues {
        host: source.host.clone(),
        open,
    }))
}

/// The issues of `read` that no task of `project` tracks now, newest first.
pub(crate) async fn untracked_now(
    user: &User,
    project: &ProjectRow,
    read: &HostIssues,
) -> Result<Vec<OpenIssue>, IssueError> {
    let tracked: Vec<u64> = user
        .project_tasks(project.id)
        .await?
        .into_iter()
        .filter(|task| task.issue_host.as_ref() == Some(&read.host))
        .filter_map(|task| task.issue.map(|issue| issue.number))
        .collect();
    Ok(untracked(read.open.clone(), &tracked))
}

/// An open pull request on a project's host, and whether it is the user's.
#[derive(Debug, Clone)]
pub(crate) struct ListedPr {
    pub(crate) pr: OpenPr,
    /// Opened by the CLI's signed-in account; false when that is unknown.
    pub(crate) mine: bool,
}

/// Open pull requests on `project`'s host, any author, newest first; empty without a host.
pub(crate) async fn list_open_prs(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Vec<ListedPr>, IssueError> {
    let Some(sources) = issue_sources(fleet, user, project).await? else {
        return Ok(Vec::new());
    };
    let (source, mut prs) = read_any(
        fleet,
        &sources,
        project,
        HostCommand::ListOpenPrs,
        parse_open_prs,
    )
    .await?;
    // Asked on the machine that listed them, whose CLI is known to work.
    let whoami = run_host(
        source.shell.as_ref(),
        &source.host,
        project,
        HostCommand::WhoAmI,
    )
    .await;
    let login = match whoami.and_then(|out| parse_login(&out)) {
        Ok(login) => Some(login),
        Err(error) => {
            log::info!(
                "projects: the host's account is unknown, so no PR counts as mine: {error:#}"
            );
            None
        }
    };
    prs.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    Ok(prs
        .into_iter()
        .map(|pr| ListedPr {
            mine: login.as_deref().is_some_and(|login| {
                !pr.author.is_empty() && login.eq_ignore_ascii_case(&pr.author)
            }),
            pr,
        })
        .collect())
}

/// The branch pull request `number` is made from, read from the host now.
pub(crate) async fn pull_request_head(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    number: u64,
) -> Result<PrHead, IssueError> {
    let Some(sources) = issue_sources(fleet, user, project).await? else {
        return Err(IssueError::Refused(
            "The project has no GitHub or Forgejo host to read pull requests from.".into(),
        ));
    };
    let (_, head) = read_any(
        fleet,
        &sources,
        project,
        HostCommand::ViewPrHead { number },
        parse_pr_head,
    )
    .await?;
    Ok(head)
}
