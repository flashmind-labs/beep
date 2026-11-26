//! A project's host as the watch asks it, through the CLI signed in on this
//! machine: `gh` for GitHub, `teacli` for Forgejo and Gitea. Each is started
//! as a program, with no shell between, so the same calls work on Windows.

use super::{
    check::{self, Answer},
    github::{self, WAIT_LIMIT},
};
use crate::{
    lock,
    process::spawn_env,
    process::{tail, tool_command},
};
use anyhow::{Context, Result, bail};
use beep_projects::repo_watch::actions::{HostRun, RunsReply, state_of};
use beep_proto::{GITHUB_ENV_VAR, RunState, SecretText, WorkflowRun};
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::{future::Future, process::Stdio, sync::Mutex, time::Duration};
use tokio::{
    io::AsyncRead,
    process::Child,
    time::{Instant, timeout},
};
use tokio_util::codec::{FramedRead, LinesCodec};

/// How long a list or a read may take.
const CALL_LIMIT: Duration = Duration::from_secs(30);
/// How long a connection to GitHub may take to open.
const CONNECT_LIMIT: Duration = Duration::from_secs(10);
/// How long the token `gh` is signed in with is kept before it is read again.
const GH_TOKEN_AGE: Duration = Duration::from_secs(10 * 60);
/// The runs a list reads, newest first.
const RUNS_PAGE: u32 = 20;
/// How often a wait asks the host, in seconds.
const WAIT_EVERY_SECS: u32 = 10;
/// The longest progress line teacli's watch prints that is read.
const MAX_LINE: usize = 256 * 1024;

/// Where a project's runs are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// `owner/name`.
    pub(crate) repo: String,
    pub(crate) host: Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Host {
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
    pub(crate) fn host_name(&self) -> &str {
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
    /// Asks a GitHub host whether the newest runs still match `etag`;
    /// `None` for another host, or with no token to ask with.
    fn check(
        &self,
        at: &Target,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<Option<Answer>>> + Send;
}

/// The CLIs on this machine, and an HTTPS client for GitHub's checks.
pub(super) struct Cli {
    /// `None` when it could not be built: no checks then.
    client: Option<Client>,
    /// The token `gh` is signed in with, as last read.
    gh_token: Mutex<Option<GhToken>>,
}

struct GhToken {
    read: Instant,
    token: Option<SecretText>,
}

impl Cli {
    pub(super) fn new() -> Self {
        let client = beep_proto::https::client()
            .user_agent(concat!("beep-node/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_LIMIT)
            .timeout(CALL_LIMIT)
            .build();
        let client = match client {
            Ok(client) => Some(client),
            Err(e) => {
                // The lists still go on their own schedule.
                log::warn!("CI watch: no HTTPS client, so no checks: {e:#}");
                None
            }
        };
        Self {
            client,
            gh_token: Mutex::new(None),
        }
    }

    /// The GitHub token: the user's from the mesh, else the one `gh` is
    /// signed in with, read again every `GH_TOKEN_AGE`.
    async fn github_token(&self) -> Option<SecretText> {
        if let Some(token) = spawn_env::secret(GITHUB_ENV_VAR) {
            return Some(token);
        }

        if let Some(cached) = lock(&self.gh_token).as_ref()
            && cached.read.elapsed() < GH_TOKEN_AGE
        {
            return cached.token.clone();
        }

        let args: Vec<String> = vec!["auth".into(), "token".into()];
        let mut command = tool_command("gh", &args);
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        let token = match timeout(CALL_LIMIT, command.output()).await {
            Ok(Ok(output)) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                (!text.is_empty()).then(|| SecretText::new(text))
            }
            // Not signed in, or no gh: no checks until it is read again.
            _ => None,
        };
        *lock(&self.gh_token) = Some(GhToken {
            read: Instant::now(),
            token: token.clone(),
        });
        token
    }

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
        let mut command = tool_command(program, &args);
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
            Host::GitHub => format!("per_page={RUNS_PAGE}"),
            Host::Forgejo { .. } => format!("limit={RUNS_PAGE}&page=1"),
        };
        let path = format!("repos/{}/actions/runs?{page}", at.repo);
        let text = Self::read(at, &path).await?;
        let reply: RunsReply = serde_json::from_str(&text).context("the run list does not read")?;
        Ok(reply.runs())
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
            Host::GitHub => github::wait(&at.repo, id).await,
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
                let mut command = tool_command("teacli", &args);
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

    async fn check(&self, at: &Target, etag: Option<&str>) -> Result<Option<Answer>> {
        let (Host::GitHub, Some(client)) = (&at.host, &self.client) else {
            return Ok(None);
        };
        let Some(token) = self.github_token().await else {
            return Ok(None);
        };
        Ok(Some(check::ask(client, &token, &at.repo, etag).await?))
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
            (Some(state), _) if state.finished() => Some(Ok(false)),
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

#[cfg(test)]
#[path = "../../tests/machine_ci_forge.rs"]
mod tests;
