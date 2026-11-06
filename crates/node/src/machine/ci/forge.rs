//! A project's host as the watch asks it, through the CLI signed in on this
//! machine: `gh` for GitHub, `teacli` for Forgejo and Gitea. Each is started
//! as a program, with no shell between, so the same calls work on Windows.

use crate::machine::provision::spawn_env::{self, Scope};
use anyhow::{Context, Result, bail};
use beep_projects::repo_watch::actions::{HostRun, RunsReply, state_of};
use beep_proto::{RunState, WorkflowRun};
use futures::StreamExt;
use serde::Deserialize;
use std::{future::Future, process::Stdio, time::Duration};
use tokio::{
    io::AsyncRead,
    process::{Child, Command},
    time::timeout,
};
use tokio_util::codec::{FramedRead, LinesCodec};

/// How long a list or a read may take.
const CALL_LIMIT: Duration = Duration::from_secs(30);
/// How long one wait on a run may take; the run is listed again after.
pub(super) const WAIT_LIMIT: Duration = Duration::from_secs(30 * 60);
/// The runs a list reads, newest first.
const RUNS_PAGE: u32 = 20;
/// How often a wait asks the host, in seconds.
const WAIT_EVERY_SECS: u32 = 10;
/// The most of a CLI's output kept for a message.
const MAX_DETAIL: usize = 300;
/// The longest progress line teacli's watch prints that is read.
const MAX_LINE: usize = 256 * 1024;

/// Where a project's runs are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Target {
    /// `owner/name`.
    pub(super) repo: String,
    pub(super) host: Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Host {
    GitHub,
    /// A Forgejo or Gitea server: its base URL, as teacli takes it, and its
    /// host name with the port, as the Actions tile names it.
    Forgejo {
        url: String,
        name: String,
    },
}

impl Target {
    /// The host's name, such as `github.com`: what its reach is kept under.
    pub(super) fn host_name(&self) -> &str {
        match &self.host {
            Host::GitHub => beep_projects::git_items::github::HOST,
            Host::Forgejo { name, .. } => name,
        }
    }
}

/// What the watch asks of a host.
pub(super) trait Forge: Send + Sync + 'static {
    /// The newest runs, newest first.
    fn list(&self, at: &Target) -> impl Future<Output = Result<Vec<WorkflowRun>>> + Send;
    /// Run `id` as it stands.
    fn one(&self, at: &Target, id: u64) -> impl Future<Output = Result<WorkflowRun>> + Send;
    /// The account the CLI is signed in as.
    fn login(&self, at: &Target) -> impl Future<Output = Result<String>> + Send;
    /// Waits until run `id` ends; true when it passed. Dropping it stops the
    /// wait and the process behind it.
    fn wait(&self, at: &Target, id: u64) -> impl Future<Output = Result<bool>> + Send;
}

/// The CLIs on this machine.
pub(super) struct Cli;

impl Cli {
    /// The program and the arguments that read `path` of the host's API.
    fn api(at: &Target, path: &str) -> (&'static str, Vec<String>) {
        match &at.host {
            Host::GitHub => ("gh", vec!["api".into(), path.into()]),
            Host::Forgejo { url, .. } => (
                "teacli",
                vec!["api".into(), path.into(), "--host".into(), url.clone()],
            ),
        }
    }

    async fn read(at: &Target, path: &str) -> Result<String> {
        let (program, args) = Self::api(at, path);
        let mut command = command(program, &args);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = timeout(CALL_LIMIT, command.output())
            .await
            .with_context(|| format!("{program} took too long"))?
            .with_context(|| format!("starting {program}"))?;
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr);
            bail!("{program} failed: {}", tail(&said));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Forge for Cli {
    async fn list(&self, at: &Target) -> Result<Vec<WorkflowRun>> {
        let page = match at.host {
            Host::GitHub => "per_page",
            Host::Forgejo { .. } => "limit",
        };
        let path = format!("repos/{}/actions/runs?{page}={RUNS_PAGE}", at.repo);
        let text = Self::read(at, &path).await?;
        let reply: RunsReply = serde_json::from_str(&text).context("the run list does not read")?;
        Ok(reply.runs().into_iter().map(|listed| listed.run).collect())
    }

    async fn one(&self, at: &Target, id: u64) -> Result<WorkflowRun> {
        let path = format!("repos/{}/actions/runs/{id}", at.repo);
        let text = Self::read(at, &path).await?;
        let run: HostRun = serde_json::from_str(&text).context("the run does not read")?;
        Ok(run.into_run())
    }

    async fn login(&self, at: &Target) -> Result<String> {
        #[derive(Deserialize)]
        struct User {
            login: String,
        }
        let text = Self::read(at, "user").await?;
        let user: User = serde_json::from_str(&text).context("the account does not read")?;
        Ok(user.login)
    }

    async fn wait(&self, at: &Target, id: u64) -> Result<bool> {
        let id = id.to_string();
        match &at.host {
            Host::GitHub => {
                let args: Vec<String> = vec![
                    "run".into(),
                    "watch".into(),
                    id,
                    "--repo".into(),
                    at.repo.clone(),
                    "--exit-status".into(),
                    "--interval".into(),
                    WAIT_EVERY_SECS.to_string(),
                ];
                let mut command = command("gh", &args);
                command.stdout(Stdio::null()).stderr(Stdio::null());
                let child = command.spawn().context("starting gh")?;
                timeout(WAIT_LIMIT, exited(child))
                    .await
                    .context("gh waited too long")?
            }
            // teacli prints the run as one JSON line each time it changes,
            // the last one with how it ended.
            Host::Forgejo { url, .. } => {
                let args: Vec<String> = vec![
                    "action".into(),
                    "watch".into(),
                    at.repo.clone(),
                    id,
                    "--host".into(),
                    url.clone(),
                    "--format".into(),
                    "json".into(),
                    "--interval".into(),
                    format!("{WAIT_EVERY_SECS}s"),
                    "--timeout".into(),
                    format!("{}m", WAIT_LIMIT.as_secs() / 60),
                ];
                let mut command = command("teacli", &args);
                command.stdout(Stdio::piped()).stderr(Stdio::null());
                let mut child = command.spawn().context("starting teacli")?;
                let stdout = child.stdout.take().context("teacli has no output")?;
                timeout(WAIT_LIMIT, async {
                    let last = last_state(stdout).await;
                    let passed = exited(child).await;
                    match last {
                        Some(RunState::Passed {}) => Ok(true),
                        Some(state) if ended(state) => Ok(false),
                        // Ended without saying how: the exit says.
                        _ => passed,
                    }
                })
                .await
                .context("teacli waited too long")?
            }
        }
    }
}

/// Whether `child` exited with success.
async fn exited(mut child: Child) -> Result<bool> {
    Ok(child
        .wait()
        .await
        .context("waiting on the watch")?
        .success())
}

/// The run's state in the last line of teacli's watch that reads, once its
/// output closes. A line too long or not JSON is passed over.
async fn last_state(output: impl AsyncRead + Unpin) -> Option<RunState> {
    #[derive(Deserialize)]
    struct Progress {
        status: String,
        #[serde(default)]
        conclusion: Option<String>,
    }
    let mut lines = FramedRead::new(output, LinesCodec::new_with_max_length(MAX_LINE));
    let mut last = None;
    while let Some(line) = lines.next().await {
        let Ok(line) = line else {
            continue;
        };
        if let Ok(progress) = serde_json::from_str::<Progress>(&line) {
            last = Some(state_of(&progress.status, progress.conclusion.as_deref()));
        }
    }
    last
}

/// Whether a run in `state` has ended.
fn ended(state: RunState) -> bool {
    matches!(
        state,
        RunState::Passed {} | RunState::Failed {} | RunState::Cancelled {} | RunState::Skipped {}
    )
}

/// `program` with `args`, the tools' folders on `PATH` and the GitHub token,
/// killed when dropped.
fn command(program: &str, args: &[String]) -> Command {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    spawn_env::apply(&mut command, Scope::Agent(None));
    command
}

/// The end of what a CLI said, on one line.
fn tail(text: &str) -> String {
    let text = text.trim().replace('\n', " ");
    let start = text
        .char_indices()
        .rev()
        .nth(MAX_DETAIL)
        .map_or(0, |(at, _)| at);
    text[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::last_state;
    use beep_proto::RunState;

    #[tokio::test]
    async fn the_watch_ends_on_its_last_progress_line() {
        let output = concat!(
            r#"{"run_id":7,"status":"running","conclusion":null,"jobs":[]}"#,
            "\n",
            "not json\n",
            r#"{"run_id":7,"status":"completed","conclusion":"failure","jobs":[]}"#,
            "\n",
        );
        assert_eq!(
            last_state(output.as_bytes()).await,
            Some(RunState::Failed {})
        );
        assert_eq!(last_state(&b""[..]).await, None);
    }
}
