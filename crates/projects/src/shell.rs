//! Where a project's commands run.

use anyhow::Result;
use async_trait::async_trait;
use secrecy::SecretString;
use std::time::Duration;
use thiserror::Error;

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

/// What a script printed and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOutput {
    /// `None` when the script did not exit with a code.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// The end of what it printed: long runs keep the part that says how they finished.
    pub output: String,
}

/// The machine could not be asked.
#[derive(Debug, Error)]
#[error("{0:#}")]
pub struct ShellError(pub anyhow::Error);

/// One machine's shell for whole scripts. The server runs them over a node's link and the
/// desktop through a peer, so the script is a [`SecretString`]: it may carry a token.
#[async_trait]
pub trait MachineShell: Send + Sync {
    async fn run(&self, script: &SecretString, limit: Duration) -> Result<ShellOutput, ShellError>;
}

/// `text` as one shell word.
pub fn quote(text: &str) -> String {
    // Only a NUL byte cannot be quoted, and no command can carry one.
    shlex::try_quote(&text.replace('\0', ""))
        .map(|quoted| quoted.into_owned())
        .unwrap_or_default()
}
