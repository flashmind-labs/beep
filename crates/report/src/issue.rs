use crate::bundle::{MachineEntry, Manifest};
use crate::redact::{RedactError, redact};
use std::env;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

const REPO: &str = "flashmind-labs/beep";
const LABEL: &str = "report";

const SHORT_ID: usize = 8;
const SIGNED_IN_LIMIT: Duration = Duration::from_secs(10);
const OPEN_LIMIT: Duration = Duration::from_secs(60);
const FIELD_MAX: usize = 40;

/// The public issue: the user's text, the reference and versions. Never logs,
/// machine names, paths or ids beyond the sender's short one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Error)]
pub enum IssueError {
    #[error("could not run gh: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("gh failed: {0}")]
    Failed(String),
    #[error("gh printed no issue URL")]
    NoUrl,
    #[error("gh did not finish in time")]
    TimedOut,
}

/// Redacts `manifest.text` itself and fails closed if it cannot.
pub fn issue(manifest: &Manifest, reference: &str) -> Result<Issue, RedactError> {
    let text = quote_mentions(&redact(&manifest.text)?);
    let mut body = String::new();

    for line in text.lines() {
        body.push_str("> ");
        body.push_str(line);
        body.push('\n');
    }

    if body.is_empty() {
        body.push_str(">\n");
    }

    let sender = manifest.sender.to_string();
    let short: String = sender.chars().take(SHORT_ID).collect();
    let _ = writeln!(body, "\nReference: {reference}");
    let _ = writeln!(body, "Device: {short}");
    let _ = writeln!(
        body,
        "App: {} {}",
        plain(&manifest.app),
        or_unknown(&plain(&manifest.app_version))
    );

    if !manifest.machines.is_empty() {
        body.push_str("\nMachines:\n");

        for machine in &manifest.machines {
            let _ = writeln!(body, "- {}", machine_line(machine));
        }
    }

    Ok(Issue {
        title: format!("Problem report {reference}"),
        body,
    })
}

/// Wraps every `@word` in backticks so the issue pings nobody.
fn quote_mentions(text: &str) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let len = tail.find(|c| !word(c)).unwrap_or(tail.len());
        let starts_word = out.chars().next_back().is_none_or(|c| !word(c));
        if len == 0 || !starts_word {
            out.push('@');
            out.push_str(&tail[..len]);
        } else {
            out.push('`');
            out.push('@');
            out.push_str(&tail[..len]);
            out.push('`');
        }
        rest = &tail[len..];
    }

    out.push_str(rest);
    out
}

fn machine_line(m: &MachineEntry) -> String {
    format!(
        "{}, {} {}, {}: {}",
        m.kind,
        plain(&m.os),
        plain(&m.os_version),
        or_unknown(&plain(&m.version)),
        m.answer.word()
    )
}

/// Remote strings reach a public page: one line of plain characters, capped.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | ' '))
        .take(FIELD_MAX)
        .collect()
}

fn or_unknown(version: &str) -> &str {
    if version.is_empty() {
        "unknown"
    } else {
        version
    }
}

/// `gh` (`gh.exe` on Windows) in a `PATH` folder.
pub fn find_gh() -> Option<PathBuf> {
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

pub async fn gh_signed_in(gh: &Path) -> bool {
    let mut command = Command::new(gh);
    command
        .args(["auth", "status", "--hostname", "github.com"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    timeout(SIGNED_IN_LIMIT, command.status())
        .await
        .is_ok_and(|status| status.is_ok_and(|status| status.success()))
}

/// Opens the issue with the user's own `gh` and returns its URL. Runs again
/// without the label when the repository has none.
pub async fn open_issue(gh: &Path, issue: &Issue) -> Result<String, IssueError> {
    timeout(OPEN_LIMIT, async {
        match create(gh, issue, true).await {
            Err(IssueError::Failed(stderr)) if label_missing(&stderr) => {
                create(gh, issue, false).await
            }
            other => other,
        }
    })
    .await
    .unwrap_or(Err(IssueError::TimedOut))
}

fn label_missing(stderr: &str) -> bool {
    let stderr = stderr.to_lowercase();
    stderr.contains("not found") && stderr.contains("label")
}

async fn create(gh: &Path, issue: &Issue, label: bool) -> Result<String, IssueError> {
    let mut command = Command::new(gh);
    command
        .args(["issue", "create", "-R", REPO, "--title", &issue.title])
        .args(["--body-file", "-"]);
    if label {
        command.args(["--label", LABEL]);
    }

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // A gh that exits early closes the pipe; its exit status says why.
        let _ = stdin.write_all(issue.body.as_bytes()).await;
    }

    let output = child.wait_with_output().await?;

    if !output.status.success() {
        return Err(IssueError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with("https://"))
        .map(str::to_string)
        .ok_or(IssueError::NoUrl)
}

#[cfg(test)]
#[path = "tests/issue.rs"]
mod tests;
