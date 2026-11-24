//! Where a project's workflow runs are read: its host's Actions API with the user's token.

use super::{
    actions::{HostRun, JobsReply, LOG_LINES, RunsReply, failing_part},
    http::{HostCalls, TailLimits, Who},
};
use crate::{
    git_items::{Caller, HostError, forks::Api, github},
    model::{ForgejoUrl, RepoName},
};
use beep_proto::{RunJob, RunLog, WorkflowRun};
use reqwest::{Client, StatusCode};
use secrecy::SecretString;
use std::sync::Arc;
use tokio::sync::Semaphore;

const GITHUB_API: &str = "https://api.github.com";
/// Runs one list reads.
const RUNS_PAGE: u32 = 20;
/// Jobs one run's list reads.
const JOBS_PAGE: u32 = 50;
/// The most of a log kept, and read, for its end.
const LOG_KEEP: usize = 64 * 1024;
const LOG_MAX: usize = 32 * 1024 * 1024;

/// A project's repository on its host, and the token that reads it.
pub struct Target {
    pub api: Api,
    /// The API base, such as `https://api.github.com` or `https://git.example.com/api/v1`.
    pub base: String,
    pub repo: RepoName,
    /// The host's name, for messages.
    pub host: String,
    pub token: SecretString,
    /// Whose cache and rate limit the calls use.
    pub who: Who,
}

impl Target {
    fn page(&self, size: u32) -> String {
        match self.api {
            Api::GitHub => format!("per_page={size}"),
            // Forgejo leaves pagination off unless a page is specified.
            Api::Forgejo => format!("limit={size}&page=1"),
        }
    }

    fn repo_url(&self) -> String {
        format!("{}/repos/{}", self.base.trim_end_matches('/'), self.repo)
    }

    /// A caller for this target's host, its calls bounded by `calls`.
    pub fn caller<'a>(&'a self, calls: &'a Arc<Semaphore>, client: &'a Client) -> Caller<'a> {
        Caller {
            client,
            calls,
            host: &self.host,
            token: &self.token,
        }
    }
}

impl Target {
    /// `repo` on GitHub, read with `token` of `account`, which names the rate limit's owner.
    pub fn github(user: &str, repo: RepoName, token: SecretString, account: &str) -> Target {
        Target {
            api: Api::GitHub,
            base: GITHUB_API.to_owned(),
            repo,
            host: github::HOST.to_owned(),
            token,
            who: Who(format!("{user} {} {account}", github::HOST)),
        }
    }

    /// `repo` on the Forgejo server at `url`, read with `token`. Whether the server is reachable
    /// from here is the caller's to check.
    pub fn forgejo(user: &str, repo: RepoName, url: &ForgejoUrl, token: SecretString) -> Target {
        Target {
            api: Api::Forgejo,
            base: format!("{}/api/v1", url.as_ref()),
            repo,
            host: url.host(),
            token,
            who: Who(format!("{user} {} ", url.host())),
        }
    }
}

/// The newest runs, newest first.
pub async fn list(
    calls: &HostCalls,
    caller: &Caller<'_>,
    target: &Target,
) -> Result<Vec<WorkflowRun>, HostError> {
    let url = format!(
        "{}/actions/runs?{}",
        target.repo_url(),
        target.page(RUNS_PAGE)
    );
    let reply: RunsReply = calls.get_json(caller, &target.who, &url).await?;
    Ok(reply.runs())
}

/// Open pull request heads, read through the same local HTTP client as runs.
pub async fn heads(
    calls: &HostCalls,
    caller: &Caller<'_>,
    target: &Target,
) -> Result<Vec<(u64, String)>, HostError> {
    #[derive(serde::Deserialize)]
    struct Head {
        #[serde(rename = "ref")]
        branch: String,
    }
    #[derive(serde::Deserialize)]
    struct Pull {
        number: u64,
        head: Head,
    }

    let url = format!(
        "{}/pulls?state=open&{}",
        target.repo_url(),
        target.page(100)
    );
    let pulls: Vec<Pull> = calls.get_json(caller, &target.who, &url).await?;
    Ok(pulls
        .into_iter()
        .map(|pull| (pull.number, pull.head.branch))
        .collect())
}

/// The run `id`.
pub async fn one(
    calls: &HostCalls,
    caller: &Caller<'_>,
    target: &Target,
    id: u64,
) -> Result<WorkflowRun, HostError> {
    let url = format!("{}/actions/runs/{id}", target.repo_url());
    let run: HostRun = calls.get_json(caller, &target.who, &url).await?;
    Ok(run.into_run())
}

/// The jobs of run `id` with their steps; `None` when the host has no jobs API (older Forgejo).
pub async fn jobs(
    calls: &HostCalls,
    caller: &Caller<'_>,
    target: &Target,
    id: u64,
) -> Result<Option<Vec<RunJob>>, HostError> {
    let url = format!(
        "{}/actions/runs/{id}/jobs?{}",
        target.repo_url(),
        target.page(JOBS_PAGE)
    );

    match calls.get_json::<JobsReply>(caller, &target.who, &url).await {
        Ok(reply) => Ok(Some(reply.jobs())),
        Err(HostError::Status { status, .. })
            if target.api == Api::Forgejo && status == StatusCode::NOT_FOUND.as_u16() =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// The end of failed `job`'s log, its failing `step`'s lines where the log marks them.
pub async fn log(
    calls: &HostCalls,
    caller: &Caller<'_>,
    target: &Target,
    job: &RunJob,
    step: Option<String>,
) -> Result<RunLog, HostError> {
    let url = format!("{}/actions/jobs/{}/logs", target.repo_url(), job.id);
    let tail = calls
        .tail(
            caller,
            &url,
            TailLimits {
                keep: LOG_KEEP,
                max: LOG_MAX,
            },
        )
        .await?;
    let cut = failing_part(&tail.text, LOG_LINES);
    Ok(RunLog {
        job: job.name.clone(),
        step: step.or(cut.step),
        text: cut.text,
        truncated: cut.truncated || tail.cut,
    })
}

#[cfg(test)]
#[path = "../tests/repo_watch_runs.rs"]
mod tests;
