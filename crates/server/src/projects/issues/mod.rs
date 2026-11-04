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
use anyhow::Result;
use beep_projects::issues::SourceError;

pub(crate) use super::commands::OpenIssue;

/// Most issues retried for tasks without one, per wake.
const MAX_RETRIES: usize = 5;
/// The most of an issue's text a task made from it keeps.
const MAX_ISSUE_TEXT: usize = 16 * 1024;
pub(crate) use beep_projects::issues::{HOST_LIMIT, head, run_rendered};

/// Runs `command` against the project's `host`, failing unless it exits 0.
pub(crate) async fn run_host(
    shell: &dyn Shell,
    host: &IssueHost,
    project: &ProjectRow,
    command: HostCommand<'_>,
) -> Result<String> {
    beep_projects::issues::run_host(shell, host, &project.name, command).await
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
    beep_projects::issues::host_failed(error).into()
}

impl From<SourceError> for IssueError {
    fn from(error: SourceError) -> Self {
        match error {
            SourceError::Refused(why) => Self::Refused(why),
            SourceError::Store(error) => Self::Store(error),
        }
    }
}

#[cfg(test)]
mod tests;
