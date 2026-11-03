//! Where a project's workflow runs are read and the calls that read them:
//! the Actions API of its GitHub or Forgejo host, with the user's token
//! for that host.

use super::{
    actions::{HostRun, JobsReply, LOG_LINES, ListedRun, RunsReply, failing_part},
    http::{HostCalls, TailLimits, Who},
};
use crate::{
    app::App,
    credentials::{Opened, Slot, github_for},
    git_items::{Caller, HostError, forks::Api, github},
    projects::model::{IssueHost, RepoName},
    protocol::{RunJob, RunLog, WorkflowRun},
    repo_watch::hosts::RepoOnHost,
    user::User,
};
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
pub(crate) struct Target {
    pub(crate) api: Api,
    /// The API's base, such as `https://api.github.com` or
    /// `https://git.example.com/api/v1`.
    pub(crate) base: String,
    pub(crate) repo: RepoName,
    /// The host's name, for messages.
    pub(crate) host: String,
    pub(crate) token: SecretString,
    /// Whose cache and rate limit the calls use.
    pub(crate) who: Who,
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
    pub(crate) fn caller<'a>(
        &'a self,
        calls: &'a Arc<Semaphore>,
        client: &'a Client,
    ) -> Caller<'a> {
        Caller {
            client,
            calls,
            host: &self.host,
            token: &self.token,
        }
    }
}

/// Where the runs of `place` are read, or why they cannot be, for the
/// user `user_id` with the git credentials `opened`.
pub(crate) async fn target(
    user_id: &str,
    opened: &[Opened],
    place: &RepoOnHost,
) -> Result<Target, String> {
    let who = |host: &str, account: &str| Who(format!("{user_id} {host} {account}"));
    match &place.host {
        IssueHost::Github => {
            let opened = github_for(opened, place.repo.owner()).ok_or_else(|| {
                "Add a GitHub token in Settings to see this project's runs.".to_owned()
            })?;
            let account = opened
                .github
                .as_ref()
                .map(|login| login.login.as_str())
                .unwrap_or_default();
            Ok(Target {
                api: Api::GitHub,
                base: GITHUB_API.to_owned(),
                repo: place.repo.clone(),
                host: github::HOST.to_owned(),
                token: opened.secret.clone(),
                who: who(github::HOST, account),
            })
        }
        IssueHost::Forgejo(name) => {
            let (opened, url) = opened
                .iter()
                .find_map(|opened| match &opened.slot {
                    Slot::Forgejo(url) if url.hostname().eq_ignore_ascii_case(name.as_str()) => {
                        Some((opened, url))
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    format!(
                        "Add a token for {} in Settings to see this project's runs.",
                        name.as_str()
                    )
                })?;
            if !url.is_public().await {
                return Err(HostError::Private(url.host()).to_string());
            }
            Ok(Target {
                api: Api::Forgejo,
                base: format!("{}/api/v1", url.as_str()),
                repo: place.repo.clone(),
                host: url.host(),
                token: opened.secret.clone(),
                who: who(&url.host(), ""),
            })
        }
    }
}

/// The user's git credentials, opened, or why they did not open.
pub(crate) async fn credentials(app: &App, user: &User) -> Result<Vec<Opened>, String> {
    user.git_credentials(&app.credential_key)
        .await
        .map_err(|e| {
            log::warn!("runs: the git credentials did not open: {e:#}");
            "Beep could not open your git tokens.".to_owned()
        })
}

/// The newest runs, newest first.
pub(crate) async fn list(
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
pub(crate) async fn one(
    calls: &HostCalls,
    caller: &Caller<'_>,
    target: &Target,
    id: u64,
) -> Result<WorkflowRun, HostError> {
    let url = format!("{}/actions/runs/{id}", target.repo_url());
    let run: HostRun = calls.get_json(caller, &target.who, &url).await?;
    Ok(run.into_run())
}

/// The jobs of run `id`, with their steps where the host gives them.
/// `None` when the host has no API for jobs, as older Forgejo servers.
pub(crate) async fn jobs(
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

/// The end of the failed `job`'s log, its failing step's lines where the
/// log marks them. `step` is the failing step's name from the job's steps.
pub(crate) async fn log(
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
    use crate::{
        protocol::RunState,
        repo_watch::test_host::{Reply, TestHost},
    };
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
