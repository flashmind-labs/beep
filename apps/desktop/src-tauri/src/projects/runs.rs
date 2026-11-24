//! A mesh project's pull requests and issues, with when they were read, and
//! its workflow runs (GitHub or Forgejo Actions): the run list with live
//! progress, one run's jobs and steps, and the end of a failed job's log. A
//! repository pushed to GitHub and a Forgejo server runs on both: each run
//! names its host.

use crate::{
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk},
    projects::threads::{IssueRow, PullRequestRow},
};
use beep_proto::{
    ApiError, HostedChecks, HostedRun, HostedRuns, LabeledProjectLists, PrChecks, ProjectRunDetail,
    RunJob, RunLog, RunProgress, RunsHost, StepProgress, WorkflowRun,
};
use serde::Serialize;
use std::time::SystemTime;
use uuid::Uuid;

fn ms(at: Option<SystemTime>) -> Option<u64> {
    at.map(epoch_ms)
}

/// The name of the mesh project `project_id` is the id of.
async fn named(fleet: &FleetState<'_>, project_id: Uuid) -> Result<String> {
    desk(fleet)
        .await?
        .project_name(project_id)
        .ok_or(DesktopError::Api(ApiError::NotFound {}))
}

/// The project's lists and when they were read (ProjectLists).
#[derive(Debug, Serialize)]
pub struct ListsRow {
    prs: Vec<PullRequestRow>,
    prs_error: Option<String>,
    issues: Vec<IssueRow>,
    issues_error: Option<String>,
}

/// The pull requests and issues as last read, at once, with a newer read
/// behind it when they are stale (the page hears of it as `project-live`);
/// `refresh` reads the host now.
#[tauri::command]
pub async fn center_project_lists(
    fleet: FleetState<'_>,
    project_id: Uuid,
    refresh: bool,
) -> Result<Option<ListsRow>> {
    let name = named(&fleet, project_id).await?;
    let LabeledProjectLists { lists, pr_labels } = fleet.lists(&name, refresh).await?;
    Ok(Some(ListsRow {
        prs: lists
            .pull_requests
            .into_iter()
            .zip(pr_labels)
            .map(|(pr, labels)| PullRequestRow::labeled(pr, labels))
            .collect(),
        prs_error: lists.prs_error,
        issues: lists.issues.into_iter().map(Into::into).collect(),
        issues_error: lists.issues_error,
    }))
}

#[derive(Debug, Serialize)]
pub struct StepRow {
    number: u32,
    of: u32,
    name: String,
    started_at: Option<u64>,
}

impl From<StepProgress> for StepRow {
    fn from(s: StepProgress) -> Self {
        Self {
            number: s.number,
            of: s.of,
            name: s.name,
            started_at: ms(s.started_at),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProgressRow {
    job: String,
    job_started_at: Option<u64>,
    jobs_done: u32,
    jobs: u32,
    step: Option<StepRow>,
}

impl From<RunProgress> for ProgressRow {
    fn from(p: RunProgress) -> Self {
        Self {
            job: p.job,
            job_started_at: ms(p.job_started_at),
            jobs_done: p.jobs_done,
            jobs: p.jobs,
            step: p.step.map(Into::into),
        }
    }
}

/// One workflow run (WorkflowRun).
#[derive(Debug, Serialize)]
pub struct RunRow {
    id: u64,
    number: u64,
    workflow: String,
    title: String,
    branch: String,
    event: String,
    actor: String,
    state: &'static str,
    url: String,
    started_at: Option<u64>,
    finished_at: Option<u64>,
    progress: Option<ProgressRow>,
    /// The host it ran on; `None` on an opened run, which names its host apart.
    host: Option<String>,
}

impl From<WorkflowRun> for RunRow {
    fn from(r: WorkflowRun) -> Self {
        Self {
            host: None,
            id: r.id,
            number: r.number,
            workflow: r.workflow,
            title: r.title,
            branch: r.branch,
            event: r.event,
            actor: r.actor,
            state: r.state.into(),
            url: r.url,
            started_at: ms(r.started_at),
            finished_at: ms(r.finished_at),
            progress: r.progress.map(Into::into),
        }
    }
}

impl From<HostedRun> for RunRow {
    fn from(r: HostedRun) -> Self {
        Self {
            host: Some(r.host),
            ..r.run.into()
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChecksRow {
    number: u64,
    state: &'static str,
    progress: Option<ProgressRow>,
    /// As in `RunRow`.
    host: Option<String>,
}

impl From<PrChecks> for ChecksRow {
    fn from(c: PrChecks) -> Self {
        Self {
            host: None,
            number: c.number,
            state: c.state.into(),
            progress: c.progress.map(Into::into),
        }
    }
}

impl From<HostedChecks> for ChecksRow {
    fn from(c: HostedChecks) -> Self {
        Self {
            host: Some(c.host),
            ..c.checks.into()
        }
    }
}

/// The project's runs (HostedRuns).
#[derive(Debug, Serialize)]
pub struct RunsRow {
    runs: Vec<RunRow>,
    checks: Vec<ChecksRow>,
    synced_at: Option<u64>,
    error: Option<String>,
    rate_limited_until: Option<u64>,
    hosts: Vec<RunsHost>,
}

impl From<HostedRuns> for RunsRow {
    fn from(r: HostedRuns) -> Self {
        let primary = r.hosts.iter().find(|h| h.primary);
        Self {
            synced_at: ms(r.hosts.iter().filter_map(|h| h.synced_at).max()),
            rate_limited_until: ms(primary.and_then(|h| h.rate_limited_until)),
            runs: r.runs.into_iter().map(RunRow::from).collect(),
            checks: r.checks.into_iter().map(ChecksRow::from).collect(),
            error: r.error,
            hosts: r.hosts,
        }
    }
}

/// The newest workflow runs with live progress and each pull request's
/// checks, from every host the repository is on.
#[tauri::command]
pub async fn center_project_runs(
    fleet: FleetState<'_>,
    project_id: Uuid,
    refresh: bool,
) -> Result<RunsRow> {
    let name = named(&fleet, project_id).await?;
    Ok(RunsRow::from(fleet.runs(&name, refresh).await?))
}

/// One run opened (ProjectRunDetail).
#[derive(Debug, Serialize)]
pub struct RunDetailRow {
    run: RunRow,
    jobs: Vec<RunJob>,
    log: Option<RunLog>,
    error: Option<String>,
    rate_limited_until: Option<u64>,
}

impl From<ProjectRunDetail> for RunDetailRow {
    fn from(d: ProjectRunDetail) -> Self {
        Self {
            run: RunRow::from(d.run),
            jobs: d.jobs,
            log: d.log,
            error: d.error,
            rate_limited_until: ms(d.rate_limited_until),
        }
    }
}

/// One run with its jobs, steps and, when it failed, the end of its log,
/// read from `host`, the host its row names; without one, from the primary
/// host. A finished run is given as read before; `refresh` reads the host
/// now.
#[tauri::command]
pub async fn center_project_run(
    fleet: FleetState<'_>,
    project_id: Uuid,
    run_id: u64,
    host: Option<String>,
    refresh: Option<bool>,
) -> Result<RunDetailRow> {
    let name = named(&fleet, project_id).await?;
    let read = fleet
        .run(&name, host.as_deref(), run_id, refresh.unwrap_or(false))
        .await?;
    let mut row: RunDetailRow = read.into();
    row.run.host = host;
    Ok(row)
}
