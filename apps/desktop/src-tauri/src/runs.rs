//! A project's pull requests and issues from the server's copy, with when it
//! was read, and its workflow runs (GitHub or Forgejo Actions): the run
//! list with live progress, one run's jobs and steps, and the end of a
//! failed job's log. A repository pushed to GitHub and a Forgejo server
//! runs on both: each run names its host, and a server from before that
//! gives the primary host's runs alone.

use crate::{
    beep::{Beep, epoch_ms},
    error::Result,
    projects::unknown_op,
    threads::{IssueRow, PullRequestRow},
};
use beep_proto::{
    GetLabeledProjectLists, GetProjectRun, GetProjectRunOn, GitHostKind, HostedChecks, HostedRun,
    HostedRuns, LabeledProjectLists, ListProjectRuns, ListProjectRunsAll, PrChecks,
    ProjectRunDetail, ProjectRuns, RunJob, RunLog, RunProgress, RunState, RunStep, RunsHost,
    StepProgress, WorkflowRun,
};
use serde::Serialize;
use std::{sync::Arc, time::SystemTime};
use tauri::State;
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

fn ms(at: Option<SystemTime>) -> Option<u64> {
    at.map(epoch_ms)
}

fn state(state: RunState) -> &'static str {
    state.into()
}

/// The project's lists and when they were read (ProjectLists).
#[derive(Debug, Serialize)]
pub struct ListsRow {
    prs: Vec<PullRequestRow>,
    prs_error: Option<String>,
    issues: Vec<IssueRow>,
    issues_error: Option<String>,
    /// In milliseconds since the Unix epoch.
    synced_at: Option<u64>,
}

/// The pull requests and issues from the server's copy; `refresh` reads the
/// host now, at most twice a minute. `None` from a server too old to keep
/// them, which the page then lists the older way.
#[tauri::command]
pub async fn center_project_lists(
    beep: Service<'_>,
    project_id: Uuid,
    refresh: bool,
) -> Result<Option<ListsRow>> {
    match beep
        .call(GetLabeledProjectLists {
            project_id,
            refresh,
        })
        .await
    {
        Ok(LabeledProjectLists { lists, pr_labels }) => Ok(Some(ListsRow {
            prs: lists
                .pull_requests
                .into_iter()
                .zip(pr_labels)
                .map(|(pr, labels)| PullRequestRow::labeled(pr, labels))
                .collect(),
            prs_error: lists.prs_error,
            issues: lists.issues.into_iter().map(Into::into).collect(),
            issues_error: lists.issues_error,
            synced_at: ms(lists.synced_at),
        })),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
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
    pull_requests: Vec<u64>,
    /// The host it ran on; `None` from a server that reads one host.
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
            state: state(r.state),
            url: r.url,
            started_at: ms(r.started_at),
            finished_at: ms(r.finished_at),
            progress: r.progress.map(Into::into),
            pull_requests: r.pull_requests,
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
    run_id: u64,
    progress: Option<ProgressRow>,
    /// As in `RunRow`.
    host: Option<String>,
}

impl From<PrChecks> for ChecksRow {
    fn from(c: PrChecks) -> Self {
        Self {
            host: None,
            number: c.number,
            state: state(c.state),
            run_id: c.run_id,
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

/// One host the runs are read from (RunsHost).
#[derive(Debug, Serialize)]
pub struct HostRow {
    host: String,
    /// `github` or `forgejo`.
    kind: &'static str,
    repo: String,
    primary: bool,
    synced_at: Option<u64>,
    error: Option<String>,
    rate_limited_until: Option<u64>,
}

impl From<RunsHost> for HostRow {
    fn from(h: RunsHost) -> Self {
        Self {
            kind: match h.kind {
                GitHostKind::GitHub {} => "github",
                GitHostKind::Forgejo {} => "forgejo",
            },
            host: h.host,
            repo: h.repo,
            primary: h.primary,
            synced_at: ms(h.synced_at),
            error: h.error,
            rate_limited_until: ms(h.rate_limited_until),
        }
    }
}

/// The project's runs (ProjectRuns, or HostedRuns with `hosts`).
#[derive(Debug, Serialize)]
pub struct RunsRow {
    runs: Vec<RunRow>,
    checks: Vec<ChecksRow>,
    synced_at: Option<u64>,
    error: Option<String>,
    rate_limited_until: Option<u64>,
    /// Empty from a server that reads one host.
    hosts: Vec<HostRow>,
}

impl From<ProjectRuns> for RunsRow {
    fn from(r: ProjectRuns) -> Self {
        Self {
            runs: r.runs.into_iter().map(Into::into).collect(),
            checks: r.checks.into_iter().map(Into::into).collect(),
            synced_at: ms(r.synced_at),
            error: r.error,
            rate_limited_until: ms(r.rate_limited_until),
            hosts: Vec::new(),
        }
    }
}

impl From<HostedRuns> for RunsRow {
    fn from(r: HostedRuns) -> Self {
        let primary = r.hosts.iter().find(|h| h.primary);
        Self {
            synced_at: ms(r.hosts.iter().filter_map(|h| h.synced_at).max()),
            rate_limited_until: ms(primary.and_then(|h| h.rate_limited_until)),
            runs: r.runs.into_iter().map(Into::into).collect(),
            checks: r.checks.into_iter().map(Into::into).collect(),
            error: r.error,
            hosts: r.hosts.into_iter().map(Into::into).collect(),
        }
    }
}

/// The newest workflow runs with live progress and each pull request's
/// checks, from every host the repository is on, or from its primary host
/// on a server that reads one; `None` from a server too old to read them.
#[tauri::command]
pub async fn center_project_runs(
    beep: Service<'_>,
    project_id: Uuid,
    refresh: bool,
) -> Result<Option<RunsRow>> {
    match beep
        .call(ListProjectRunsAll {
            project_id,
            refresh,
        })
        .await
    {
        Ok(runs) => return Ok(Some(runs.into())),
        Err(e) if unknown_op(&e) => {}
        Err(e) => return Err(e),
    }
    match beep
        .call(ListProjectRuns {
            project_id,
            refresh,
        })
        .await
    {
        Ok(runs) => Ok(Some(runs.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Serialize)]
pub struct RunStepRow {
    number: u32,
    name: String,
    state: &'static str,
    started_at: Option<u64>,
    finished_at: Option<u64>,
}

impl From<RunStep> for RunStepRow {
    fn from(s: RunStep) -> Self {
        Self {
            number: s.number,
            name: s.name,
            state: state(s.state),
            started_at: ms(s.started_at),
            finished_at: ms(s.finished_at),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RunJobRow {
    id: u64,
    name: String,
    state: &'static str,
    started_at: Option<u64>,
    finished_at: Option<u64>,
    steps: Vec<RunStepRow>,
    url: String,
}

impl From<RunJob> for RunJobRow {
    fn from(j: RunJob) -> Self {
        Self {
            id: j.id,
            name: j.name,
            state: state(j.state),
            started_at: ms(j.started_at),
            finished_at: ms(j.finished_at),
            steps: j.steps.into_iter().map(Into::into).collect(),
            url: j.url,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RunLogRow {
    job: String,
    step: Option<String>,
    text: String,
    truncated: bool,
}

impl From<RunLog> for RunLogRow {
    fn from(l: RunLog) -> Self {
        Self {
            job: l.job,
            step: l.step,
            text: l.text,
            truncated: l.truncated,
        }
    }
}

/// One run opened (ProjectRunDetail).
#[derive(Debug, Serialize)]
pub struct RunDetailRow {
    run: RunRow,
    jobs: Vec<RunJobRow>,
    log: Option<RunLogRow>,
    error: Option<String>,
    rate_limited_until: Option<u64>,
}

impl From<ProjectRunDetail> for RunDetailRow {
    fn from(d: ProjectRunDetail) -> Self {
        Self {
            run: d.run.into(),
            jobs: d.jobs.into_iter().map(Into::into).collect(),
            log: d.log.map(Into::into),
            error: d.error,
            rate_limited_until: ms(d.rate_limited_until),
        }
    }
}

/// One run with its jobs, steps and, when it failed, the end of its log,
/// read from `host`, the host its row names. Without one, or from a server
/// that reads one host, from the primary host.
#[tauri::command]
pub async fn center_project_run(
    beep: Service<'_>,
    project_id: Uuid,
    run_id: u64,
    host: Option<String>,
) -> Result<RunDetailRow> {
    if let Some(host) = host {
        match beep
            .call(GetProjectRunOn {
                project_id,
                host: host.clone(),
                run_id,
            })
            .await
        {
            Ok(detail) => {
                let mut row: RunDetailRow = detail.into();
                row.run.host = Some(host);
                return Ok(row);
            }
            Err(e) if unknown_op(&e) => {}
            Err(e) => return Err(e),
        }
    }
    let detail = beep.call(GetProjectRun { project_id, run_id }).await?;
    Ok(detail.into())
}
