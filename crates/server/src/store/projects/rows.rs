//! The project tables' rows and how they are read.

use crate::{
    projects::model::{
        IssueHost, IssueRef, MAX_MACHINE_WORKERS, MAX_PROJECT_WORKERS, MAX_USER_WORKERS,
        MachinePath, ProjectId, RepoName, Roles, StartRequest, TaskId, TaskSlug, TaskState,
        Visibility, WorkerAgent, WorkerId, WorkerState,
    },
    protocol::{TaskCounts, node::Capability},
};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// A project as the store keeps it.
#[derive(Debug, Clone)]
pub(crate) struct ProjectRow {
    pub(crate) id: ProjectId,
    pub(crate) name: RepoName,
    pub(crate) roles: Roles,
    pub(crate) visibility: Visibility,
    pub(crate) default_branch: String,
    /// The machine its workers start on first.
    pub(crate) pin: Option<EndpointId>,
}

/// Where a project is checked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckoutRow {
    pub(crate) machine: EndpointId,
    pub(crate) path: MachinePath,
    /// The checkout outputs are copied back to.
    pub(crate) home: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskRow {
    pub(crate) id: TaskId,
    pub(crate) project: ProjectId,
    pub(crate) slug: TaskSlug,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) state: TaskState,
    pub(crate) depends_on: Option<TaskId>,
    pub(crate) needs: Vec<Capability>,
    pub(crate) issue_host: Option<IssueHost>,
    pub(crate) issue: Option<IssueRef>,
    pub(crate) branch: Option<String>,
    pub(crate) pr_url: Option<String>,
    /// How it was asked to start, while it waits for a slot.
    pub(crate) start: Option<StartRequest>,
    /// The PR's fingerprint as last reported.
    pub(crate) pr_seen: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    /// Someone else opened the issue: Beep never closes, relabels or edits it, and comments at most once after asking.
    pub(crate) issue_foreign: bool,
    pub(crate) issue_commented: bool,
}

/// Who opened the issue a task tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssueAuthor {
    /// The user's own host account, kept in step like the issues Beep files.
    User,
    /// Anyone else: Beep leaves it as it is.
    Other,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkerRow {
    pub(crate) id: WorkerId,
    pub(crate) task: TaskId,
    pub(crate) project: ProjectId,
    pub(crate) machine: EndpointId,
    pub(crate) agent: WorkerAgent,
    /// The node's id for its session, once it has started.
    pub(crate) session: Option<String>,
    pub(crate) folder: MachinePath,
    pub(crate) state: WorkerState,
    /// Whether it waits on an answer to a question it asked.
    pub(crate) asking: bool,
    pub(crate) last_output: Option<String>,
    pub(crate) persona: Option<String>,
    /// How many of its session's turns the project chat has heard about.
    pub(crate) turns_seen: u32,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) last_turn_at: Option<DateTime<Utc>>,
    pub(crate) ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub(crate) struct NewTask {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) depends_on: Option<After>,
    pub(crate) needs: Vec<Capability>,
    /// How to start it, when it should start once made.
    pub(crate) start: Option<StartRequest>,
}

/// The task a new one waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum After {
    /// A task the project has.
    Task(TaskId),
    /// One made earlier in the same call, by its index there.
    Earlier(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct NewWorker {
    pub(crate) task: TaskId,
    pub(crate) machine: EndpointId,
    pub(crate) agent: WorkerAgent,
    pub(crate) folder: MachinePath,
    pub(crate) persona: Option<String>,
    pub(crate) persona_commit: Option<String>,
}

/// Why a worker did not start, told to the model as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum StartRefused {
    #[error("{MAX_PROJECT_WORKERS} workers already run in this project")]
    ProjectFull,
    #[error("{MAX_MACHINE_WORKERS} workers already run on this machine")]
    MachineFull,
    #[error("{MAX_USER_WORKERS} workers already run across your projects")]
    UserFull,
    #[error("another worker already writes in that folder on this machine")]
    FolderTaken,
    #[error("this task already has a worker")]
    TaskHasWorker,
}

/// What is still open in a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenCounts {
    /// Tasks not done or dropped.
    pub(crate) tasks: u32,
    pub(crate) workers: u32,
}

/// One project summed up, as [`Store::project_overviews`] gives it.
#[derive(Debug, Clone)]
pub(crate) struct OverviewRow {
    pub(crate) id: ProjectId,
    /// `org/repo`.
    pub(crate) name: String,
    pub(crate) tasks: TaskCounts,
    pub(crate) workers: u32,
    /// The task that changed last, dropped ones left out.
    pub(crate) latest: Option<TaskRow>,
}

/// What [`Store::track_issue`] did.
#[derive(Debug, Clone)]
pub(crate) enum TrackedIssue {
    /// A new task tracks the issue.
    Made(TaskRow),
    /// This task of the project tracked it already.
    Already(TaskRow),
}

pub(super) fn endpoint(text: &str) -> Result<EndpointId> {
    text.parse()
        .with_context(|| format!("stored machine {text} is not an endpoint id"))
}

pub(super) fn issue_host(text: Option<String>) -> Result<Option<IssueHost>> {
    text.map(|text| {
        IssueHost::from_stored(&text).ok_or_else(|| anyhow!("unknown issue host {text}"))
    })
    .transpose()
}

pub(super) fn project_from(row: &PgRow) -> Result<ProjectRow> {
    let name: String = row.try_get(1)?;
    let visibility: String = row.try_get(4)?;
    let pin: Option<String> = row.try_get(6)?;
    Ok(ProjectRow {
        id: ProjectId(row.try_get(0)?),
        name: RepoName::parse(&name)?,
        roles: Roles {
            issues: issue_host(row.try_get(2)?)?,
            prs: issue_host(row.try_get(3)?)?,
        },
        visibility: visibility.parse()?,
        default_branch: row.try_get(5)?,
        pin: pin.as_deref().map(endpoint).transpose()?,
    })
}

pub(super) fn checkout_from(row: &PgRow) -> Result<CheckoutRow> {
    let machine: String = row.try_get(0)?;
    let path: String = row.try_get(1)?;
    Ok(CheckoutRow {
        machine: endpoint(&machine)?,
        path: MachinePath::parse(&path)?,
        home: row.try_get(2)?,
    })
}

pub(super) fn task_from(row: &PgRow) -> Result<TaskRow> {
    let slug: String = row.try_get(2)?;
    let state: String = row.try_get(5)?;
    let depends_on: Option<Uuid> = row.try_get(6)?;
    let needs: Vec<String> = row.try_get(7)?;
    let issue_number: Option<i64> = row.try_get(9)?;
    let issue_url: Option<String> = row.try_get(10)?;
    let start: Option<serde_json::Value> = row.try_get(13)?;
    let issue = match (issue_number, issue_url) {
        (Some(number), Some(url)) => Some(IssueRef {
            number: u64::try_from(number)?,
            url,
        }),
        _ => None,
    };
    Ok(TaskRow {
        id: TaskId(row.try_get(0)?),
        project: ProjectId(row.try_get(1)?),
        slug: TaskSlug::parse(&slug).ok_or_else(|| anyhow!("stored slug {slug} is not one"))?,
        title: row.try_get(3)?,
        body: row.try_get(4)?,
        state: state.parse()?,
        depends_on: depends_on.map(TaskId),
        needs: needs
            .iter()
            .map(|need| need.parse())
            .collect::<Result<_, _>>()?,
        issue_host: issue_host(row.try_get(8)?)?,
        issue,
        branch: row.try_get(11)?,
        pr_url: row.try_get(12)?,
        start: start.map(serde_json::from_value).transpose()?,
        pr_seen: row.try_get(14)?,
        created_at: row.try_get(15)?,
        updated_at: row.try_get(16)?,
        issue_foreign: row.try_get(17)?,
        issue_commented: row.try_get(18)?,
    })
}

pub(in crate::store) fn worker_from(row: &PgRow) -> Result<WorkerRow> {
    let machine: String = row.try_get(3)?;
    let agent: String = row.try_get(4)?;
    let folder: String = row.try_get(6)?;
    let state: String = row.try_get(7)?;
    let turns_seen: i32 = row.try_get(12)?;
    Ok(WorkerRow {
        id: WorkerId(row.try_get(0)?),
        task: TaskId(row.try_get(1)?),
        project: ProjectId(row.try_get(2)?),
        machine: endpoint(&machine)?,
        agent: agent.parse()?,
        session: row.try_get(5)?,
        folder: MachinePath::parse(&folder)?,
        state: state.parse()?,
        asking: row.try_get(8)?,
        last_output: row.try_get(9)?,
        persona: row.try_get(10)?,
        turns_seen: u32::try_from(turns_seen)?,
        started_at: row.try_get(13)?,
        last_turn_at: row.try_get(14)?,
        ended_at: row.try_get(15)?,
    })
}
