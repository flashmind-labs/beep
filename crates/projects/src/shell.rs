//! Where a project's commands run.

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// A machine's shell, one command at a time.
#[async_trait]
pub trait Shell: Send + Sync {
    async fn run(&self, command: &str, limit: Duration) -> Result<CommandOutput>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// `None` when the command did not finish.
    pub code: Option<i32>,
    /// The first 64 KiB of what it printed.
    pub out: String,
}

/// `text` as one shell word.
pub fn quote(text: &str) -> String {
    // Only a NUL byte cannot be quoted, and no command can carry one.
    shlex::try_quote(&text.replace('\0', ""))
        .map(|quoted| quoted.into_owned())
        .unwrap_or_default()
}
