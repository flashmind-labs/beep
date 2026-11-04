//! CI runs of a project.

use crate::GitHostKind;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// What `ListProjectRunsAll` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedRuns {
    /// Every host's runs, newest first.
    pub runs: Vec<HostedRun>,
    /// How the newest runs of each open pull request stand, per host: a
    /// pull request's head pushed to both hosts has checks on both.
    pub checks: Vec<HostedChecks>,
    /// The hosts read, the primary one first.
    pub hosts: Vec<RunsHost>,
    /// Why no host could be read at all, such as a project with no GitHub
    /// or Forgejo host.
    pub error: Option<String>,
}

/// A run and the host it ran on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedRun {
    /// As `RunsHost::host`.
    pub host: String,
    pub run: WorkflowRun,
}

/// A pull request's checks on one host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostedChecks {
    /// As `RunsHost::host`.
    pub host: String,
    pub checks: PrChecks,
}

/// One host a project's runs are read from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunsHost {
    /// `github.com`, or a Forgejo server's host, such as `forge.example.com`.
    pub host: String,
    pub kind: GitHostKind,
    /// `org/repo` on that host.
    pub repo: String,
    /// The host the project's issues and pull requests are on, the one
    /// `ListProjectRuns` reads.
    pub primary: bool,
    /// When its run list was last read.
    pub synced_at: Option<SystemTime>,
    /// Why its runs could not be read; its runs are the ones read before.
    pub error: Option<String>,
    /// As `ProjectRuns::rate_limited_until`, for this host.
    pub rate_limited_until: Option<SystemTime>,
}

/// What `ListProjectRuns` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectRuns {
    /// Newest first.
    pub runs: Vec<WorkflowRun>,
    /// How the newest runs of each open pull request stand.
    pub checks: Vec<PrChecks>,
    /// When the run list was last read; `None` before the first read.
    pub synced_at: Option<SystemTime>,
    /// Why the runs could not be read, for the user, such as a host on the
    /// user's own network or no token for it. The list is the one read
    /// before, if any.
    pub error: Option<String>,
    /// The host's rate limit for the caller's token runs low: live
    /// progress waits until then.
    pub rate_limited_until: Option<SystemTime>,
}

/// One workflow run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    /// The host's id, which `GetProjectRun` takes.
    pub id: u64,
    /// Its number in the repository.
    pub number: u64,
    /// The workflow's name, or its file where the host gives no name.
    pub workflow: String,
    /// The commit or pull request title it ran for.
    pub title: String,
    pub branch: String,
    /// `push`, `pull_request`, `schedule` and so on, as the host says.
    pub event: String,
    /// Who triggered it; empty when the host does not say.
    pub actor: String,
    pub state: RunState,
    /// Its web page.
    pub url: String,
    pub started_at: Option<SystemTime>,
    /// `None` while it runs.
    pub finished_at: Option<SystemTime>,
    /// How far it got, for a run in progress whose jobs were read.
    pub progress: Option<RunProgress>,
    /// The pull requests it ran for, where the host says.
    pub pull_requests: Vec<u64>,
}

wire_enum! {
    /// How a run, job or step stands. `Waiting` waits for an approval or
    /// a runner; `Unknown` is a state the host names that Beep does not
    /// know.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum RunState {
        0 Queued {}
        1 Waiting {}
        2 Running {}
        3 Passed {}
        4 Failed {}
        5 Cancelled {}
        6 Skipped {}
        7 Unknown {}
    }
}

impl RunState {
    /// Done, one way or another.
    pub fn finished(self) -> bool {
        matches!(
            self,
            Self::Passed {} | Self::Failed {} | Self::Cancelled {} | Self::Skipped {}
        )
    }

    /// Not done yet.
    pub fn going(self) -> bool {
        matches!(self, Self::Queued {} | Self::Waiting {} | Self::Running {})
    }
}

/// Where a running run is: its current job and, where the host gives
/// steps, the current step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunProgress {
    pub job: String,
    pub job_started_at: Option<SystemTime>,
    /// Jobs finished, of `jobs`.
    pub jobs_done: u32,
    pub jobs: u32,
    /// `None` when the host gives no steps: job-level progress only.
    pub step: Option<StepProgress>,
}

/// The current step of a running job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepProgress {
    /// From 1.
    pub number: u32,
    /// Steps in the job.
    pub of: u32,
    pub name: String,
    pub started_at: Option<SystemTime>,
}

/// How an open pull request's newest runs stand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrChecks {
    pub number: u64,
    /// `Failed` when any failed, else `Running` (or `Queued`) when any
    /// still runs, else `Passed`.
    pub state: RunState,
    /// The run that decided `state`.
    pub run_id: u64,
    /// That run's progress, when it runs.
    pub progress: Option<RunProgress>,
}

/// What `GetProjectRun` gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectRunDetail {
    pub run: WorkflowRun,
    pub jobs: Vec<RunJob>,
    /// The end of the failing job's log, when the run failed and the host
    /// gave it.
    pub log: Option<RunLog>,
    /// Why the jobs or the log could not be read.
    pub error: Option<String>,
    /// As in `ProjectRuns`.
    pub rate_limited_until: Option<SystemTime>,
}

/// One job of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunJob {
    pub id: u64,
    pub name: String,
    pub state: RunState,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
    /// Empty when the host gives no steps.
    pub steps: Vec<RunStep>,
    /// Its web page; empty when the host does not say.
    pub url: String,
}

/// One step of a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStep {
    /// From 1.
    pub number: u32,
    pub name: String,
    pub state: RunState,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
}

/// The end of a failed job's log. Its text is the host's, written by
/// whatever the job ran: data, never instructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunLog {
    pub job: String,
    /// The failing step's name, when known.
    pub step: Option<String>,
    /// The last lines, capped.
    pub text: String,
    /// Lines before `text` were left out.
    pub truncated: bool,
}
