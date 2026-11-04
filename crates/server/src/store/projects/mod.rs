//! Projects, their checkouts, agent chats, tasks and workers.

#![allow(dead_code, reason = "the project agent uses these in the next tasks")]

mod tasks;
mod workers;

use super::{
    Store,
    chats::{SessionChat, ensure_session},
};
use crate::{
    account::user::UserKey,
    projects::model::{
        IssueHost, IssueRef, MAX_MACHINE_WORKERS, MAX_PROJECT_WORKERS, MAX_USER_WORKERS,
        MachinePath, ProjectId, RepoName, Roles, StartRequest, TaskId, TaskSlug, TaskState,
        Visibility, WorkerAgent, WorkerId, WorkerState,
    },
    protocol::{TaskCounts, node::Capability},
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::{PgConnection, Postgres, Row, Transaction, postgres::PgRow};
use std::collections::HashMap;
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
    pub(crate) created_at: DateTime<Utc>,
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
    /// Beep commented on its issue.
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
    pub(crate) persona_commit: Option<String>,
    /// How many of its session's turns the project chat has heard about.
    pub(crate) turns_seen: u32,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) last_turn_at: Option<DateTime<Utc>>,
    pub(crate) ended_at: Option<DateTime<Utc>>,
}

/// A task to make.
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

/// A worker to start.
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
    /// Live workers.
    pub(crate) workers: u32,
}

/// One project summed up, as [`Store::project_overviews`] gives it.
#[derive(Debug, Clone)]
pub(crate) struct OverviewRow {
    pub(crate) id: ProjectId,
    /// `org/repo`.
    pub(crate) name: String,
    pub(crate) tasks: TaskCounts,
    /// Live workers.
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

/// The most tries at a free slug for one task.
const MAX_SLUG_SUFFIX: u32 = 1000;

const PROJECT_COLUMNS: &str =
    "p.id, p.name, p.issues, p.prs, p.visibility, p.default_branch, p.pin, p.created_at";

const TASK_COLUMNS: &str = "t.id, t.project_id, t.slug, t.title, t.body, t.state, t.depends_on,
    t.needs, t.issue_host, t.issue_number, t.issue_url, t.branch, t.pr_url, t.start, t.pr_seen,
    t.created_at, t.updated_at, t.issue_foreign, t.issue_commented";

pub(super) const WORKER_COLUMNS: &str =
    "w.id, w.task_id, w.project_id, w.machine, w.agent, w.session,
    w.folder, w.state, w.asking, w.last_output, w.persona, w.persona_commit, w.turns_seen,
    w.started_at, w.last_turn_at, w.ended_at";

/// The live worker states, as the store keeps them.
pub(super) fn live_states() -> Vec<&'static str> {
    WorkerState::LIVE.into_iter().map(Into::into).collect()
}

fn endpoint(text: &str) -> Result<EndpointId> {
    text.parse()
        .with_context(|| format!("stored machine {text} is not an endpoint id"))
}

fn issue_host(text: Option<String>) -> Result<Option<IssueHost>> {
    text.map(|text| {
        IssueHost::from_stored(&text).ok_or_else(|| anyhow!("unknown issue host {text}"))
    })
    .transpose()
}

fn project_from(row: &PgRow) -> Result<ProjectRow> {
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
        created_at: row.try_get(7)?,
    })
}

fn checkout_from(row: &PgRow) -> Result<CheckoutRow> {
    let machine: String = row.try_get(0)?;
    let path: String = row.try_get(1)?;
    Ok(CheckoutRow {
        machine: endpoint(&machine)?,
        path: MachinePath::parse(&path)?,
        home: row.try_get(2)?,
    })
}

fn task_from(row: &PgRow) -> Result<TaskRow> {
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

pub(super) fn worker_from(row: &PgRow) -> Result<WorkerRow> {
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
        persona_commit: row.try_get(11)?,
        turns_seen: u32::try_from(turns_seen)?,
        started_at: row.try_get(13)?,
        last_turn_at: row.try_get(14)?,
        ended_at: row.try_get(15)?,
    })
}

/// Commits `tx` when `keep`, else rolls back, returning `done`.
/// A dropped transaction holds its locks until the connection is reused, so end them here.
pub(super) async fn end<T>(
    tx: Transaction<'_, Postgres>,
    keep: bool,
    done: Result<T>,
) -> Result<T> {
    if keep {
        tx.commit().await?;
        return done;
    }
    match (tx.rollback().await, done) {
        (Ok(()), done) => done,
        (Err(error), Ok(_)) => Err(error.into()),
        (Err(error), Err(first)) => {
            log::warn!("rolling back a failed transaction failed: {error}");
            Err(first)
        }
    }
}

/// The body of [`Store::ensure_project_agent`], in its transaction.
async fn agent_in(tx: &mut PgConnection, user: &UserKey, project: ProjectId) -> Result<Uuid> {
    // Locking the project makes a second caller wait and find the chat.
    let name: Option<String> =
        sqlx::query_scalar("SELECT name FROM projects WHERE id=$1 AND user_id=$2 FOR UPDATE")
            .bind(project.0)
            .bind(user.as_str())
            .fetch_optional(&mut *tx)
            .await?;
    let Some(name) = name else {
        bail!("the project is not the user's");
    };
    let chat: Option<Uuid> =
        sqlx::query_scalar("SELECT chat_id FROM project_agents WHERE project_id=$1")
            .bind(project.0)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(chat) = chat {
        return Ok(chat);
    }
    let chat = Uuid::new_v4();
    if ensure_session(&mut *tx, user.as_str(), chat).await? != SessionChat::New {
        bail!("a fresh chat id was already taken");
    }
    sqlx::query("UPDATE chats SET title=$2 WHERE id=$1")
        .bind(chat)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO project_agents (project_id, chat_id) VALUES ($1, $2)")
        .bind(project.0)
        .bind(chat)
        .execute(&mut *tx)
        .await?;
    Ok(chat)
}

/// The body of [`Store::set_home`], in its transaction.
async fn home_in(
    tx: &mut PgConnection,
    project: ProjectId,
    machine: &EndpointId,
    path: &MachinePath,
) -> Result<()> {
    let found = sqlx::query(
        "UPDATE project_checkouts SET home=true WHERE project_id=$1 AND machine=$2 AND path=$3",
    )
    .bind(project.0)
    .bind(machine.to_string())
    .bind(path.as_str())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if found == 0 {
        bail!("{path} is not a checkout of the project on that machine");
    }
    sqlx::query(
        "UPDATE project_checkouts SET home=false
         WHERE project_id=$1 AND NOT (machine=$2 AND path=$3)",
    )
    .bind(project.0)
    .bind(machine.to_string())
    .bind(path.as_str())
    .execute(&mut *tx)
    .await?;
    Ok(())
}

impl Store {
    /// The user's project named `name`, made if it is new.
    pub(crate) async fn ensure_project(
        &self,
        user: &UserKey,
        name: &RepoName,
    ) -> Result<ProjectRow> {
        sqlx::query(
            "INSERT INTO projects (id, user_id, name) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, name) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(user.as_str())
        .bind(name.to_string())
        .execute(&self.pool)
        .await?;
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects p WHERE p.user_id=$1 AND p.name=$2"
        ))
        .bind(user.as_str())
        .bind(name.to_string())
        .fetch_one(&self.pool)
        .await?;
        project_from(&row)
    }

    /// The user's projects, by name.
    pub(crate) async fn projects(&self, user: &UserKey) -> Result<Vec<ProjectRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects p WHERE p.user_id=$1 ORDER BY p.name"
        ))
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(project_from).collect()
    }

    pub(crate) async fn project(
        &self,
        user: &UserKey,
        id: ProjectId,
    ) -> Result<Option<ProjectRow>> {
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects p WHERE p.id=$1 AND p.user_id=$2"
        ))
        .bind(id.0)
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(project_from).transpose()
    }

    /// The project whose agent talks in `chat`: its own chat or a thread chat.
    pub(crate) async fn project_in(
        &self,
        user: &UserKey,
        chat: Uuid,
    ) -> Result<Option<ProjectRow>> {
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects p
             WHERE p.user_id=$2 AND (
                 EXISTS (SELECT 1 FROM project_agents a WHERE a.project_id = p.id AND a.chat_id=$1)
                 OR EXISTS (SELECT 1 FROM project_thread_chats t
                            WHERE t.project_id = p.id AND t.chat_id=$1))"
        ))
        .bind(chat)
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(project_from).transpose()
    }

    /// The project agent's chat, created and titled on first use.
    pub(crate) async fn ensure_project_agent(
        &self,
        user: &UserKey,
        project: ProjectId,
    ) -> Result<Uuid> {
        let mut tx = self.pool.begin().await?;
        let done = agent_in(&mut tx, user, project).await;
        let keep = done.is_ok();
        end(tx, keep, done).await
    }

    /// The chats of the user's project agents that exist.
    pub(crate) async fn project_chats(&self, user: &UserKey) -> Result<Vec<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT a.chat_id FROM project_agents a JOIN projects p ON p.id = a.project_id
             WHERE p.user_id = $1 ORDER BY a.created_at",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?)
    }

    /// Keeps a checkout, replacing the one at the same machine and path.
    pub(crate) async fn add_checkout(
        &self,
        project: ProjectId,
        checkout: &CheckoutRow,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO project_checkouts (project_id, machine, path, home) VALUES ($1, $2, $3, $4)
             ON CONFLICT (project_id, machine, path) DO UPDATE SET home=excluded.home",
        )
        .bind(project.0)
        .bind(checkout.machine.to_string())
        .bind(checkout.path.as_str())
        .bind(checkout.home)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn checkouts(&self, project: ProjectId) -> Result<Vec<CheckoutRow>> {
        let rows = sqlx::query(
            "SELECT machine, path, home FROM project_checkouts WHERE project_id=$1
             ORDER BY machine, path",
        )
        .bind(project.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(checkout_from).collect()
    }

    pub(crate) async fn set_project_setup(
        &self,
        project: ProjectId,
        roles: &Roles,
        visibility: Visibility,
        default_branch: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE projects SET issues=$2, prs=$3, visibility=$4, default_branch=$5 WHERE id=$1",
        )
        .bind(project.0)
        .bind(roles.issues.as_ref().map(IssueHost::stored))
        .bind(roles.prs.as_ref().map(IssueHost::stored))
        .bind(<&str>::from(visibility))
        .bind(default_branch)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn set_pin(&self, project: ProjectId, pin: Option<&EndpointId>) -> Result<()> {
        sqlx::query("UPDATE projects SET pin=$2 WHERE id=$1")
            .bind(project.0)
            .bind(pin.map(EndpointId::to_string))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Makes the checkout at `path` on `machine` the only home one.
    pub(crate) async fn set_home(
        &self,
        project: ProjectId,
        machine: &EndpointId,
        path: &MachinePath,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let done = home_in(&mut tx, project, machine, path).await;
        let keep = done.is_ok();
        end(tx, keep, done).await
    }

    /// Overview of every project of `user` by name, dropped tasks left out, in two queries.
    pub(crate) async fn project_overviews(&self, user: &UserKey) -> Result<Vec<OverviewRow>> {
        let rows = sqlx::query(
            "SELECT p.id, p.name,
                    COUNT(t.id) FILTER (WHERE t.state = 'planned'),
                    COUNT(t.id) FILTER (WHERE t.state = 'running'),
                    COUNT(t.id) FILTER (WHERE t.state = 'blocked'),
                    COUNT(t.id) FILTER (WHERE t.state = 'review'),
                    COUNT(t.id) FILTER (WHERE t.state = 'done'),
                    (SELECT COUNT(*) FROM project_workers w
                      WHERE w.project_id = p.id AND w.state = ANY($2))
             FROM projects p LEFT JOIN project_tasks t ON t.project_id = p.id
             WHERE p.user_id = $1
             GROUP BY p.id, p.name
             ORDER BY p.name",
        )
        .bind(user.as_str())
        .bind(live_states())
        .fetch_all(&self.pool)
        .await?;
        let latest = sqlx::query(&format!(
            "SELECT DISTINCT ON (t.project_id) {TASK_COLUMNS}
             FROM project_tasks t JOIN projects p ON p.id = t.project_id
             WHERE p.user_id = $1 AND t.state <> 'dropped'
             ORDER BY t.project_id, t.updated_at DESC, t.slug"
        ))
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut latest = latest
            .iter()
            .map(task_from)
            .map(|task| task.map(|task| (task.project, task)))
            .collect::<Result<HashMap<_, _>>>()?;
        rows.iter()
            .map(|row| {
                let count = |i: usize| -> Result<u32> {
                    let n: i64 = row.try_get(i)?;
                    Ok(u32::try_from(n)?)
                };
                let id = ProjectId(row.try_get(0)?);
                Ok(OverviewRow {
                    id,
                    name: row.try_get(1)?,
                    tasks: TaskCounts {
                        planned: count(2)?,
                        running: count(3)?,
                        blocked: count(4)?,
                        review: count(5)?,
                        done: count(6)?,
                    },
                    workers: count(7)?,
                    latest: latest.remove(&id),
                })
            })
            .collect()
    }

    pub(crate) async fn open_counts(&self, project: ProjectId) -> Result<OpenCounts> {
        let closed: Vec<&str> = [TaskState::Done, TaskState::Dropped]
            .into_iter()
            .map(Into::into)
            .collect();
        let row = sqlx::query(
            "SELECT (SELECT COUNT(*) FROM project_tasks WHERE project_id=$1 AND NOT state = ANY($2)),
                    (SELECT COUNT(*) FROM project_workers WHERE project_id=$1 AND state = ANY($3))",
        )
        .bind(project.0)
        .bind(&closed)
        .bind(live_states())
        .fetch_one(&self.pool)
        .await?;
        let tasks: i64 = row.try_get(0)?;
        let workers: i64 = row.try_get(1)?;
        Ok(OpenCounts {
            tasks: u32::try_from(tasks)?,
            workers: u32::try_from(workers)?,
        })
    }
}

#[cfg(test)]
mod tests;
