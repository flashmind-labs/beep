//! Mirroring tasks to issues on the project's host, and tracking or hiding untracked open issues.

mod filing;
mod listing;
mod source;
mod track;
pub(crate) use filing::{file_issue, issue_body, mirror_missing};
#[cfg(test)]
pub(crate) use listing::list_open;
pub(crate) use listing::{
    HostIssues, ListedPr, list_open_prs, pull_request_head, read_open, untracked_now,
};
pub(crate) use source::{
    HostAndMachines, IssueSource, IssueWork, host_and_machines, issue_host, issue_source,
};
use source::{issue_sources, read_any};
pub(crate) use track::{dismiss, track};

use super::{
    commands::{HostCommand, Shell},
    model::IssueHost,
};
use crate::store::{ProjectRow, TaskRow};
use anyhow::{Result, bail};
use std::time::Duration;

pub(crate) use super::commands::OpenIssue;

/// Most issues retried for tasks without one, per wake.
const MAX_RETRIES: usize = 5;
/// The most of an issue's text a task made from it keeps.
const MAX_ISSUE_TEXT: usize = 16 * 1024;
/// How long one host command may take.
pub(crate) const HOST_LIMIT: Duration = Duration::from_secs(30);

/// Runs `command` against the project's `host`, failing unless it exits 0.
pub(crate) async fn run_host(
    shell: &dyn Shell,
    host: &IssueHost,
    project: &ProjectRow,
    command: HostCommand<'_>,
) -> Result<String> {
    run_rendered(shell, &command.render(host, &project.name)).await
}

/// Runs the host command `rendered`, failing unless it exits 0.
pub(crate) async fn run_rendered(shell: &dyn Shell, rendered: &str) -> Result<String> {
    let out = shell.run(rendered, HOST_LIMIT).await?;
    match out.code {
        Some(0) => Ok(out.out),
        code => {
            let tail: String = out
                .out
                .chars()
                .rev()
                .take(400)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            bail!(
                "the host command exited with {}: {}",
                code.map_or("no code".to_owned(), |code| code.to_string()),
                tail.trim()
            )
        }
    }
}

/// Why listing or tracking open issues failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum IssueError {
    /// For the user, saying why.
    #[error("{0}")]
    Refused(String),
    /// This task tracks the issue already.
    #[error("{} already tracks this issue.", .0.slug)]
    Tracked(Box<TaskRow>),
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

fn host_failed(error: &anyhow::Error) -> IssueError {
    IssueError::Refused(format!("Could not read the issues host: {error:#}"))
}

/// The start of `text`, at most `max` bytes, cut on a character boundary.
pub(super) fn head(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests;
