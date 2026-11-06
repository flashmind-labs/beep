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
        match &at.host {
            // The watch's exit says it ended, `gh run view` how.
            Host::GitHub => super::github::wait(&at.repo, id).await,
            // teacli prints the run as one JSON line each time it changes,
            // the last one with how it ended.
            Host::Forgejo { url, .. } => {
                let args: Vec<String> = vec![
                    "action".into(),
                    "watch".into(),
                    at.repo.clone(),
                    id.to_string(),
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
                    let watched = read_watch(stdout).await;
                    let passed = exited(child).await;
                    watched.outcome().unwrap_or(passed)
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

/// What teacli's watch printed: the last snapshot of the run, and the error
/// line it ends with when the run did not pass or the watch failed.
#[derive(Debug, Default, PartialEq)]
struct Watched {
    last: Option<RunState>,
    error: Option<WatchError>,
}

#[derive(Debug, PartialEq, Deserialize)]
struct WatchError {
    #[serde(default)]
    code: u32,
    #[serde(default)]
    message: String,
}

impl Watched {
    /// teacli's code for a run that finished without passing.
    const FINISHED: u32 = 1;

    /// True when the run passed, false when it finished otherwise, an
    /// error when the watch failed, `None` when only the exit can say.
    fn outcome(self) -> Option<Result<bool>> {
        match (self.last, self.error) {
            (Some(RunState::Passed {}), _) => Some(Ok(true)),
            (Some(state), _) if ended(state) => Some(Ok(false)),
            (_, Some(error)) if error.code == Self::FINISHED => Some(Ok(false)),
            (_, Some(error)) => Some(Err(anyhow::anyhow!(
                "teacli failed: {}",
                tail(&error.message)
            ))),
            (_, None) => None,
        }
    }
}

/// Reads teacli's watch until its output closes: one JSON snapshot of the
/// run per line, or an error line. A line too long or not JSON is passed
/// over.
async fn read_watch(output: impl AsyncRead + Unpin) -> Watched {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Line {
        Error {
            #[allow(dead_code)]
            error: bool,
            #[serde(flatten)]
            said: WatchError,
        },
        Snapshot {
            status: String,
            #[serde(default)]
            conclusion: Option<String>,
        },
    }
    let mut lines = FramedRead::new(output, LinesCodec::new_with_max_length(MAX_LINE));
    let mut watched = Watched::default();
    while let Some(line) = lines.next().await {
        let Ok(line) = line else {
            continue;
        };
        match serde_json::from_str::<Line>(&line) {
            Ok(Line::Snapshot { status, conclusion }) => {
                watched.last = Some(state_of(&status, conclusion.as_deref()));
            }
            Ok(Line::Error { said, .. }) => watched.error = Some(said),
            Err(_) => {}
        }
    }
    watched
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
pub(super) fn command(program: &str, args: &[String]) -> Command {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    spawn_env::apply(&mut command, Scope::Agent(None));
    command
}

/// The end of what a CLI said, on one line.
pub(super) fn tail(text: &str) -> String {
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
    use super::{WatchError, Watched, read_watch};
    use beep_proto::RunState;

    const RUNNING: &str = r#"{"run_id":7,"status":"running","conclusion":null,"workflow":"CI","branch":"main","event":"push","actor":"someone","url":"","started_at":"","updated_at":"","jobs":[]}"#;

    #[tokio::test]
    async fn the_last_snapshot_says_how_the_run_ended() {
        let output = format!(
            "{RUNNING}\nnot json\n{}\n{}\n",
            r#"{"run_id":7,"status":"completed","conclusion":"failure","jobs":[]}"#,
            r#"{"error":true,"code":1,"message":"workflow run 7 finished: failure"}"#,
        );
        let watched = read_watch(output.as_bytes()).await;
        assert_eq!(watched.last, Some(RunState::Failed {}));
        assert!(matches!(watched.outcome(), Some(Ok(false))));
        let passed = r#"{"run_id":7,"status":"success","conclusion":"success","jobs":[]}"#;
        let watched = read_watch(passed.as_bytes()).await;
        assert!(matches!(watched.outcome(), Some(Ok(true))));
    }

    #[tokio::test]
    async fn an_error_line_is_a_finish_only_with_code_one() {
        let finished = format!(
            "{RUNNING}\n{}\n",
            r#"{"error":true,"code":1,"message":"workflow run 7 finished: cancelled"}"#
        );
        let watched = read_watch(finished.as_bytes()).await;
        assert_eq!(
            watched.error,
            Some(WatchError {
                code: 1,
                message: "workflow run 7 finished: cancelled".into()
            })
        );
        assert!(matches!(watched.outcome(), Some(Ok(false))));
        let failed = r#"{"error":true,"code":6,"message":"API error"}"#;
        assert!(matches!(
            read_watch(failed.as_bytes()).await.outcome(),
            Some(Err(_))
        ));
        assert_eq!(read_watch(&b""[..]).await, Watched::default());
    }
}
