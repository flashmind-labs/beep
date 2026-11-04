//! Where a project's workflow runs are read: its host's Actions API with the user's token.

use super::{
    actions::{HostRun, JobsReply, LOG_LINES, ListedRun, RunsReply, failing_part},
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
            Api::Forgejo => format!("limit={size}"),
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
            base: format!("{}/api/v1", url.as_str()),
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
) -> Result<Vec<ListedRun>, HostError> {
    let url = format!(
        "{}/actions/runs?{}",
        target.repo_url(),
        target.page(RUNS_PAGE)
    );
    let reply: RunsReply = calls.get_json(caller, &target.who, &url).await?;
    Ok(reply.runs())
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
mod tests {
    use super::*;
    use crate::repo_watch::test_host::{Reply, TestHost};
    use beep_proto::RunState;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    fn test_target(api: Api, base: &str) -> Target {
        Target {
            api,
            base: base.to_owned(),
            repo: RepoName::parse("acme/widget").expect("a repository name"),
            host: "git.example.com".to_owned(),
            token: SecretString::from("tok"),
            who: Who("alice git.example.com".to_owned()),
        }
    }

    const RUNS: &str = r#"{"total_count": 1, "workflow_runs": [
      {"id": 77, "name": "CI", "run_number": 5, "event": "push", "status": "in_progress",
       "head_branch": "main", "actor": {"login": "some-user"}}]}"#;

    const JOBS: &str = r#"{"jobs": [{"id": 3, "name": "Build", "status": "in_progress",
      "steps": [{"name": "Set up job", "status": "completed", "conclusion": "success", "number": 1},
                {"name": "Run tests", "status": "in_progress", "number": 2},
                {"name": "Complete job", "status": "queued", "number": 3}]}]}"#;

    #[tokio::test]
    async fn runs_and_jobs_read_with_etags_and_steps() {
        let host = TestHost::start(|seen| match seen.path.as_str() {
            "/repos/acme/widget/actions/runs?per_page=20" => {
                if seen.header("if-none-match").as_deref() == Some("\"r1\"") {
                    Reply::status("304 Not Modified")
                } else {
                    Reply::ok(RUNS).with("etag", "\"r1\"")
                }
            }
            "/repos/acme/widget/actions/runs/77/jobs?per_page=50" => Reply::ok(JOBS),
            _ => Reply::status("404 Not Found"),
        })
        .await;
        let target = test_target(Api::GitHub, &host.base);
        let client = Client::new();
        let limit = Arc::new(Semaphore::new(2));
        let caller = Caller {
            client: &client,
            calls: &limit,
            host: &target.host,
            token: &target.token,
        };
        let calls = HostCalls::default();
        let first = list(&calls, &caller, &target).await.expect("the runs");
        let again = list(&calls, &caller, &target).await.expect("the runs");
        assert_eq!(first, again);
        assert_eq!(first[0].run.state, RunState::Running {});
        let jobs = jobs(&calls, &caller, &target, 77)
            .await
            .expect("the jobs")
            .expect("GitHub gives jobs");
        let now = crate::repo_watch::actions::progress(&jobs).expect("a job runs");
        let step = now.step.expect("with steps");
        assert_eq!(
            (step.number, step.of, step.name.as_str()),
            (2, 3, "Run tests")
        );
        let seen = host.seen();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[1].header("if-none-match").as_deref(), Some("\"r1\""));
    }

    #[tokio::test]
    async fn a_forgejo_without_a_jobs_api_gives_no_jobs_not_an_error() {
        let host = TestHost::start(|_| Reply::status("404 Not Found")).await;
        let target = test_target(Api::Forgejo, &host.base);
        let client = Client::new();
        let limit = Arc::new(Semaphore::new(2));
        let caller = Caller {
            client: &client,
            calls: &limit,
            host: &target.host,
            token: &target.token,
        };
        let calls = HostCalls::default();
        let got = jobs(&calls, &caller, &target, 12).await.expect("no error");
        assert_eq!(got, None);
        assert_eq!(
            host.seen()[0].path,
            "/repos/acme/widget/actions/runs/12/jobs?limit=50"
        );
        // GitHub's 404 is an error: the run is gone.
        let github = test_target(Api::GitHub, &host.base);
        assert!(jobs(&calls, &caller, &github, 12).await.is_err());
    }
}
