//! Mirroring tasks to issues on the project's host, and tracking or hiding untracked open issues.

mod filing;
mod track;
pub(crate) use filing::{file_issue, issue_body, mirror_missing};
pub(crate) use track::{dismiss, track};

use super::model::WorkerAgent;
use crate::store::TaskRow;
use beep_projects::issues::SourceError;
use iroh::EndpointId;

/// Most issues retried for tasks without one, per wake.
const MAX_RETRIES: usize = 5;
/// The most of an issue's text a task made from it keeps.
const MAX_ISSUE_TEXT: usize = 16 * 1024;
/// How the user asked a tracked issue's worker to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IssueWork {
    pub(crate) agent: WorkerAgent,
    /// `None` picks a machine at start.
    pub(crate) machine: Option<EndpointId>,
}

/// Why listing or tracking open issues failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum IssueError {
    #[error(transparent)]
    Source(#[from] SourceError),
    /// This task tracks the issue already.
    #[error("{} already tracks this issue.", .0.slug)]
    Tracked(Box<TaskRow>),
}

impl From<anyhow::Error> for IssueError {
    fn from(error: anyhow::Error) -> Self {
        Self::Source(SourceError::Store(error))
    }
}

#[cfg(test)]
mod tests;
