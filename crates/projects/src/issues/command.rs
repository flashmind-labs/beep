//! Running one command on an issues host through a machine's shell.

use crate::{Shell, model::IssueHost};
use anyhow::Result;
use iroh::EndpointId;
use std::{sync::Arc, time::Duration};
use thiserror::Error;

/// How long one host command may take.
pub const HOST_LIMIT: Duration = Duration::from_secs(30);

/// Why a host could not be read.
#[derive(Debug, Error)]
pub enum SourceError {
    /// For the user, saying why.
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

/// A host command that did not exit 0.
#[derive(Debug, Clone, Error)]
#[error("the host command exited with {}: {tail}", code.map_or("no code".to_owned(), |code| code.to_string()))]
pub struct HostExit {
    /// `None` when it was killed or timed out.
    pub code: Option<i32>,
    /// The end of what it printed.
    pub tail: String,
}

/// A host command that ran longer than [`HOST_LIMIT`].
#[derive(Debug, Clone, Copy, Error)]
#[error("the host command timed out after {} seconds", HOST_LIMIT.as_secs())]
pub struct HostTimeout;

/// Where a project's open issues are read: its host, via a shell on a machine with the project.
pub struct IssueSource {
    pub host: IssueHost,
    pub machine: EndpointId,
    pub shell: Arc<dyn Shell>,
}

/// Runs the host command `rendered`, failing unless it exits 0.
pub async fn run_rendered(shell: &dyn Shell, rendered: &str) -> Result<String> {
    let out = shell.run(rendered, HOST_LIMIT).await?;

    if out.timed_out {
        return Err(HostTimeout.into());
    }

    match out.exit_code {
        Some(0) => Ok(out.output),
        code => {
            let tail: String = out
                .output
                .chars()
                .rev()
                .take(400)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Err(HostExit {
                code,
                tail: tail.trim().to_owned(),
            }
            .into())
        }
    }
}
