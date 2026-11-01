//! Projects, their checkouts, agent chats, tasks and workers.

#![allow(dead_code, reason = "the project agent uses these in the next tasks")]

use super::{
    Store,
    chats::{SessionChat, ensure_session},
};
use crate::{
    projects::model::{
        IssueHost, IssueRef, MAX_MACHINE_WORKERS, MAX_PROJECT_WORKERS, MAX_USER_WORKERS,
        MachinePath, ProjectId, RepoName, Roles, StartRequest, TaskId, TaskSlug, TaskState,
        Visibility, WorkerAgent, WorkerId, WorkerState,
    },
    protocol::node::Capability,
    user::UserKey,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::{PgConnection, Postgres, Row, Transaction, postgres::PgRow};
use std::collections::HashSet;
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

/// The most tries at a free slug for one task.
const MAX_SLUG_SUFFIX: u32 = 1000;

const PROJECT_COLUMNS: &str =
    "p.id, p.name, p.issues, p.prs, p.visibility, p.default_branch, p.pin, p.created_at";

const TASK_COLUMNS: &str = "t.id, t.project_id, t.slug, t.title, t.body, t.state, t.depends_on,
    t.needs, t.issue_host, t.issue_number, t.issue_url, t.branch, t.pr_url, t.start, t.pr_seen,
    t.created_at, t.updated_at";

const WORKER_COLUMNS: &str = "w.id, w.task_id, w.project_id, w.machine, w.agent, w.session,
    w.folder, w.state, w.asking, w.last_output, w.persona, w.persona_commit, w.turns_seen,
    w.started_at, w.last_turn_at, w.ended_at";

/// The live worker states, as the store keeps them.
fn live_states() -> Vec<&'static str> {
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
    })
}

fn worker_from(row: &PgRow) -> Result<WorkerRow> {
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

/// The refusal a unique violation on one of the worker locks stands for.
fn lock_refusal(error: &sqlx::Error) -> Option<StartRefused> {
    let sqlx::Error::Database(error) = error else {
        return None;
    };
    match error.constraint()? {
        "project_workers_folder" => Some(StartRefused::FolderTaken),
        "project_workers_task" => Some(StartRefused::TaskHasWorker),
        _ => None,
    }
}

/// Commits `tx` when `keep`, else rolls it back, and returns `done`.
/// Transactions end here rather than on drop: a dropped one rolls back only
/// when its connection is next used, and holds its locks until then. When
/// `done` already failed, a failed rollback is logged and `done`'s error
/// returned.
async fn end<T>(tx: Transaction<'_, Postgres>, keep: bool, done: Result<T>) -> Result<T> {
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

/// The body of [`Store::create_tasks`], in its transaction.
async fn tasks_in(
    tx: &mut PgConnection,
    project: ProjectId,
    tasks: &[NewTask],
) -> Result<Vec<TaskRow>> {
    // Locking the project keeps two callers from picking the same slug.
    sqlx::query("SELECT 1 FROM projects WHERE id=$1 FOR UPDATE")
        .bind(project.0)
        .fetch_optional(&mut *tx)
        .await?
        .context("no such project")?;
    let mut taken: HashSet<String> =
        sqlx::query_scalar("SELECT slug FROM project_tasks WHERE project_id=$1")
            .bind(project.0)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
    let mut made = Vec::with_capacity(tasks.len());
    for task in tasks {
        let base = TaskSlug::from_title(&task.title);
        let mut slug = base.clone();
        let mut n = 1;
        while taken.contains(slug.as_str()) {
            n += 1;
            if n > MAX_SLUG_SUFFIX {
                bail!("no free slug for {base}");
            }
            slug = base.suffixed(n);
        }
        taken.insert(slug.as_str().to_owned());
        let depends_on = match task.depends_on {
            None => None,
            Some(After::Task(id)) => Some(id),
            Some(After::Earlier(at)) => Some(
                made.get(at)
                    .map(|earlier: &TaskRow| earlier.id)
                    .with_context(|| format!("task {} waits for a later one", made.len() + 1))?,
            ),
        };
        let needs: Vec<&'static str> = task.needs.iter().map(|&need| need.into()).collect();
        let row = sqlx::query(&format!(
            "INSERT INTO project_tasks AS t
                 (id, project_id, slug, title, body, depends_on, needs, start)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {TASK_COLUMNS}"
        ))
        .bind(Uuid::new_v4())
        .bind(project.0)
        .bind(slug.as_str())
        .bind(task.title.trim())
        .bind(task.body.trim())
        .bind(depends_on.map(|id| id.0))
        .bind(&needs)
        .bind(task.start.as_ref().map(serde_json::to_value).transpose()?)
        .fetch_one(&mut *tx)
        .await?;
        made.push(task_from(&row)?);
    }
    Ok(made)
}

/// The body of [`Store::start_worker`], in its transaction.
async fn start_in(
    tx: &mut PgConnection,
    user: &UserKey,
    worker: &NewWorker,
) -> Result<Result<WorkerRow, StartRefused>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(user.as_str())
        .execute(&mut *tx)
        .await?;
    let project: Option<Uuid> = sqlx::query_scalar(
        "SELECT t.project_id FROM project_tasks t JOIN projects p ON p.id = t.project_id
         WHERE t.id=$1 AND p.user_id=$2",
    )
    .bind(worker.task.0)
    .bind(user.as_str())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(project) = project else {
        bail!("the task is not the user's");
    };
    let machine = worker.machine.to_string();
    let live = live_states();
    // Count, then insert: the advisory lock keeps other starts of this
    // user out in between.
    let counts = sqlx::query(
        "SELECT COUNT(*) FILTER (WHERE project_id=$2), COUNT(*) FILTER (WHERE machine=$3),
                COUNT(*)
         FROM project_workers WHERE user_id=$1 AND state = ANY($4)",
    )
    .bind(user.as_str())
    .bind(project)
    .bind(&machine)
    .bind(&live)
    .fetch_one(&mut *tx)
    .await?;
    let in_project: i64 = counts.try_get(0)?;
    let on_machine: i64 = counts.try_get(1)?;
    let for_user: i64 = counts.try_get(2)?;
    if in_project >= MAX_PROJECT_WORKERS {
        return Ok(Err(StartRefused::ProjectFull));
    }
    if on_machine >= MAX_MACHINE_WORKERS {
        return Ok(Err(StartRefused::MachineFull));
    }
    if for_user >= MAX_USER_WORKERS {
        return Ok(Err(StartRefused::UserFull));
    }
    let inserted = sqlx::query(&format!(
        "INSERT INTO project_workers AS w
             (id, task_id, project_id, user_id, machine, agent, folder, persona, persona_commit)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING {WORKER_COLUMNS}"
    ))
    .bind(Uuid::new_v4())
    .bind(worker.task.0)
    .bind(project)
    .bind(user.as_str())
    .bind(&machine)
    .bind(<&str>::from(worker.agent))
    .bind(worker.folder.as_str())
    .bind(worker.persona.as_deref())
    .bind(worker.persona_commit.as_deref())
    .fetch_one(&mut *tx)
    .await;
    let row = match inserted {
        Ok(row) => row,
        Err(error) => match lock_refusal(&error) {
            Some(refused) => return Ok(Err(refused)),
            None => return Err(error.into()),
        },
    };
    Ok(Ok(worker_from(&row)?))
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

    /// The project whose agent talks in `chat`, if it is one.
    pub(crate) async fn project_in(
        &self,
        user: &UserKey,
        chat: Uuid,
    ) -> Result<Option<ProjectRow>> {
        let row = sqlx::query(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects p
             JOIN project_agents a ON a.project_id = p.id
             WHERE a.chat_id=$1 AND p.user_id=$2"
        ))
        .bind(chat)
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(project_from).transpose()
    }

    /// The chat of the project's agent, made and titled with the project's
    /// name on first use.
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

    /// Makes `tasks` in `project`, in order. A slug the project already
    /// has gets `-2`, `-3` and so on.
    pub(crate) async fn create_tasks(
        &self,
        project: ProjectId,
        tasks: &[NewTask],
    ) -> Result<Vec<TaskRow>> {
        let mut tx = self.pool.begin().await?;
        let done = tasks_in(&mut tx, project, tasks).await;
        let keep = done.is_ok();
        end(tx, keep, done).await
    }

    /// The project's tasks, oldest first.
    pub(crate) async fn tasks(&self, project: ProjectId) -> Result<Vec<TaskRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM project_tasks t WHERE t.project_id=$1
             ORDER BY t.created_at, t.slug"
        ))
        .bind(project.0)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(task_from).collect()
    }

    pub(crate) async fn task(&self, user: &UserKey, id: TaskId) -> Result<Option<TaskRow>> {
        let row = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM project_tasks t JOIN projects p ON p.id = t.project_id
             WHERE t.id=$1 AND p.user_id=$2"
        ))
        .bind(id.0)
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(task_from).transpose()
    }

    pub(crate) async fn task_by_slug(
        &self,
        project: ProjectId,
        slug: &str,
    ) -> Result<Option<TaskRow>> {
        let row = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM project_tasks t WHERE t.project_id=$1 AND t.slug=$2"
        ))
        .bind(project.0)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(task_from).transpose()
    }

    pub(crate) async fn set_task_state(&self, id: TaskId, state: TaskState) -> Result<()> {
        sqlx::query("UPDATE project_tasks SET state=$2, updated_at=now() WHERE id=$1")
            .bind(id.0)
            .bind(<&str>::from(state))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn set_task_issue(
        &self,
        id: TaskId,
        host: &IssueHost,
        issue: &IssueRef,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE project_tasks SET issue_host=$2, issue_number=$3, issue_url=$4, updated_at=now()
             WHERE id=$1",
        )
        .bind(id.0)
        .bind(host.stored())
        .bind(i64::try_from(issue.number)?)
        .bind(&issue.url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Sets or clears the task it waits for.
    pub(crate) async fn set_task_after(&self, id: TaskId, after: Option<TaskId>) -> Result<()> {
        sqlx::query("UPDATE project_tasks SET depends_on=$2, updated_at=now() WHERE id=$1")
            .bind(id.0)
            .bind(after.map(|after| after.0))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn set_task_branch(&self, id: TaskId, branch: &str) -> Result<()> {
        sqlx::query("UPDATE project_tasks SET branch=$2, updated_at=now() WHERE id=$1")
            .bind(id.0)
            .bind(branch)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn set_task_pr(&self, id: TaskId, url: &str) -> Result<()> {
        sqlx::query("UPDATE project_tasks SET pr_url=$2, updated_at=now() WHERE id=$1")
            .bind(id.0)
            .bind(url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Keeps how the task was asked to start, or clears it once it has.
    pub(crate) async fn set_task_start(
        &self,
        id: TaskId,
        start: Option<&StartRequest>,
    ) -> Result<()> {
        let start = start.map(serde_json::to_value).transpose()?;
        sqlx::query("UPDATE project_tasks SET start=$2, updated_at=now() WHERE id=$1")
            .bind(id.0)
            .bind(start)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn set_pr_seen(&self, id: TaskId, fingerprint: &str) -> Result<()> {
        sqlx::query("UPDATE project_tasks SET pr_seen=$2 WHERE id=$1")
            .bind(id.0)
            .bind(fingerprint)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Changes the title, the body or both. The slug stays.
    pub(crate) async fn update_task_text(
        &self,
        id: TaskId,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE project_tasks SET title=COALESCE($2, title), body=COALESCE($3, body),
                 updated_at=now()
             WHERE id=$1",
        )
        .bind(id.0)
        .bind(title.map(str::trim))
        .bind(body.map(str::trim))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The user's tasks waiting for a slot to start, oldest first.
    pub(crate) async fn queued_tasks(&self, user: &UserKey) -> Result<Vec<TaskRow>> {
        let waiting: Vec<&str> = [TaskState::Planned, TaskState::Blocked]
            .into_iter()
            .map(Into::into)
            .collect();
        let rows = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM project_tasks t JOIN projects p ON p.id = t.project_id
             WHERE p.user_id=$1 AND t.start IS NOT NULL AND t.state = ANY($2)
             ORDER BY t.created_at, t.slug"
        ))
        .bind(user.as_str())
        .bind(&waiting)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(task_from).collect()
    }

    /// Starts a worker on the task if the caps and locks allow it. Starts
    /// for one user run one at a time, so two cannot both take the last
    /// slot.
    pub(crate) async fn start_worker(
        &self,
        user: &UserKey,
        worker: &NewWorker,
    ) -> Result<Result<WorkerRow, StartRefused>> {
        // The transaction runs in its own task, so it ends even when the
        // caller is cancelled: a transaction dropped half way holds the
        // advisory lock, and so every other start of the user, until its
        // connection is next used.
        let pool = self.pool.clone();
        let user = user.clone();
        let worker = worker.clone();
        tokio::spawn(async move {
            let mut tx = pool.begin().await?;
            let started = start_in(&mut tx, &user, &worker).await;
            let keep = matches!(started, Ok(Ok(_)));
            end(tx, keep, started).await
        })
        .await
        .context("the worker start stopped")?
    }

    pub(crate) async fn set_worker_session(&self, id: WorkerId, session: &str) -> Result<()> {
        sqlx::query("UPDATE project_workers SET session=$2 WHERE id=$1")
            .bind(id.0)
            .bind(session)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Moves the worker to `state`, noting when it ended. An ended or failed
    /// worker never comes back to life, so a live `state` only moves a live
    /// worker. Says whether it moved.
    pub(crate) async fn set_worker_state(&self, id: WorkerId, state: WorkerState) -> Result<bool> {
        let moved = sqlx::query(
            "UPDATE project_workers SET state=$2,
                 ended_at = CASE WHEN $3 THEN COALESCE(ended_at, now()) ELSE ended_at END
             WHERE id=$1 AND ($3 OR state = ANY($4))",
        )
        .bind(id.0)
        .bind(<&str>::from(state))
        .bind(!state.is_live())
        .bind(live_states())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(moved == 1)
    }

    /// Notes a finished turn: the worker waits, not asking anything, with
    /// `turns` of its session's turns heard about. An ended worker stays
    /// ended, and a turn no later than one already noted changes nothing, so
    /// a late report cannot undo newer news. Says whether it was noted.
    pub(crate) async fn worker_turn(
        &self,
        id: WorkerId,
        turns: u32,
        last_output: &str,
        at: DateTime<Utc>,
    ) -> Result<bool> {
        let noted = sqlx::query(
            "UPDATE project_workers SET state=$2, asking=false, turns_seen=$3, last_output=$4,
                 last_turn_at=$5
             WHERE id=$1 AND state = ANY($6) AND turns_seen < $3",
        )
        .bind(id.0)
        .bind(<&str>::from(WorkerState::Waiting))
        .bind(i32::try_from(turns)?)
        .bind(last_output)
        .bind(at)
        .bind(live_states())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(noted == 1)
    }

    pub(crate) async fn set_worker_asking(&self, id: WorkerId, asking: bool) -> Result<()> {
        sqlx::query("UPDATE project_workers SET asking=$2 WHERE id=$1")
            .bind(id.0)
            .bind(asking)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The user's live workers on `machine`, oldest first.
    pub(crate) async fn live_workers_on(
        &self,
        user: &UserKey,
        machine: &EndpointId,
    ) -> Result<Vec<WorkerRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {WORKER_COLUMNS} FROM project_workers w
             WHERE w.user_id=$1 AND w.machine=$2 AND w.state = ANY($3) ORDER BY w.started_at"
        ))
        .bind(user.as_str())
        .bind(machine.to_string())
        .bind(live_states())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(worker_from).collect()
    }

    /// The user's live workers, oldest first.
    pub(crate) async fn live_workers(&self, user: &UserKey) -> Result<Vec<WorkerRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {WORKER_COLUMNS} FROM project_workers w
             WHERE w.user_id=$1 AND w.state = ANY($2) ORDER BY w.started_at"
        ))
        .bind(user.as_str())
        .bind(live_states())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(worker_from).collect()
    }

    /// The project's live workers and those that ended since `since`,
    /// oldest first.
    pub(crate) async fn workers(
        &self,
        project: ProjectId,
        since: DateTime<Utc>,
    ) -> Result<Vec<WorkerRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {WORKER_COLUMNS} FROM project_workers w
             WHERE w.project_id=$1 AND (w.state = ANY($2) OR w.ended_at >= $3)
             ORDER BY w.started_at"
        ))
        .bind(project.0)
        .bind(live_states())
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(worker_from).collect()
    }

    pub(crate) async fn worker(&self, user: &UserKey, id: WorkerId) -> Result<Option<WorkerRow>> {
        let row = sqlx::query(&format!(
            "SELECT {WORKER_COLUMNS} FROM project_workers w WHERE w.id=$1 AND w.user_id=$2"
        ))
        .bind(id.0)
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(worker_from).transpose()
    }

    /// Fails every worker still starting, at startup: the server that was
    /// starting them is gone, so they never got a session.
    pub(crate) async fn fail_starting_workers(&self) -> Result<Vec<WorkerRow>> {
        let rows = sqlx::query(&format!(
            "UPDATE project_workers AS w SET state=$2, ended_at=now() WHERE state=$1
             RETURNING {WORKER_COLUMNS}"
        ))
        .bind(<&str>::from(WorkerState::Starting))
        .bind(<&str>::from(WorkerState::Failed))
        .fetch_all(&self.pool)
        .await?;
        let mut failed: Vec<WorkerRow> = rows.iter().map(worker_from).collect::<Result<_>>()?;
        failed.sort_by_key(|worker| worker.started_at);
        Ok(failed)
    }

    /// The user id of the project's owner.
    pub(crate) async fn project_owner(&self, project: ProjectId) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT user_id FROM projects WHERE id=$1")
                .bind(project.0)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// The user id of the worker's owner.
    pub(crate) async fn worker_owner(&self, worker: WorkerId) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT user_id FROM project_workers WHERE id=$1")
                .bind(worker.0)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Every project, of any user, with a task the check timer looks at.
    pub(crate) async fn projects_wanting_checks(&self) -> Result<Vec<ProjectId>> {
        let states: Vec<&str> = [TaskState::Running, TaskState::Review]
            .into_iter()
            .map(Into::into)
            .collect();
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT project_id FROM project_tasks WHERE state = ANY($1)",
        )
        .bind(&states)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids.into_iter().map(ProjectId).collect())
    }

    /// Every waiting worker, of any user.
    pub(crate) async fn waiting_workers(&self) -> Result<Vec<WorkerRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {WORKER_COLUMNS} FROM project_workers w WHERE w.state=$1"
        ))
        .bind(<&str>::from(WorkerState::Waiting))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(worker_from).collect()
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
mod tests {
    use super::*;
    use crate::{projects::model::InputFile, store::TestDb};
    use chrono::Duration as TimeDelta;
    use iroh::SecretKey;

    fn machine() -> EndpointId {
        SecretKey::generate().public()
    }

    fn repo(name: &str) -> RepoName {
        RepoName::parse(name).expect("a valid repo name")
    }

    fn path(text: &str) -> MachinePath {
        MachinePath::parse(text).expect("a valid path")
    }

    fn new_task(title: &str) -> NewTask {
        NewTask {
            title: title.into(),
            body: String::new(),
            depends_on: None,
            needs: Vec::new(),
            start: None,
        }
    }

    /// Makes `count` tasks in `project`.
    async fn tasks_in(store: &Store, project: ProjectId, count: usize) -> Vec<TaskId> {
        let titles: Vec<NewTask> = (0..count).map(|n| new_task(&format!("Task {n}"))).collect();
        store
            .create_tasks(project, &titles)
            .await
            .expect("tasks are made")
            .into_iter()
            .map(|task| task.id)
            .collect()
    }

    fn worker_for(task: TaskId, machine: EndpointId, folder: &str) -> NewWorker {
        NewWorker {
            task,
            machine,
            agent: WorkerAgent::Claude,
            folder: path(folder),
            persona: None,
            persona_commit: None,
        }
    }

    async fn start(
        store: &Store,
        user: &UserKey,
        worker: &NewWorker,
    ) -> Result<WorkerRow, StartRefused> {
        store
            .start_worker(user, worker)
            .await
            .expect("the store answers")
    }

    #[tokio::test]
    async fn a_project_and_its_chat_are_made_once() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let made = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let again = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        assert_eq!(made.id, again.id);
        assert_eq!(made.visibility, Visibility::Public);
        assert_eq!(made.default_branch, "main");
        assert_eq!(store.projects(&user).await.unwrap().len(), 1);
        // Another user's project of the same name is theirs.
        let bob = UserKey::for_test("bob");
        let theirs = store
            .ensure_project(&bob, &repo("acme/widget"))
            .await
            .unwrap();
        assert_ne!(theirs.id, made.id);
        assert!(store.project(&bob, made.id).await.unwrap().is_none());

        let chat = store.ensure_project_agent(&user, made.id).await.unwrap();
        assert_eq!(
            store.ensure_project_agent(&user, made.id).await.unwrap(),
            chat
        );
        let found = store.project_in(&user, chat).await.unwrap().unwrap();
        assert_eq!(found.id, made.id);
        assert!(store.project_in(&bob, chat).await.unwrap().is_none());
        assert!(store.ensure_project_agent(&bob, made.id).await.is_err());
        let title: Option<String> = sqlx::query_scalar("SELECT title FROM chats WHERE id=$1")
            .bind(chat)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(title.as_deref(), Some("acme/widget"));
    }

    #[tokio::test]
    async fn setup_and_checkouts_are_kept() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let build_box = machine();
        let gpu_box = machine();
        for (machine, home) in [(build_box, true), (gpu_box, false)] {
            let checkout = CheckoutRow {
                machine,
                path: path("/srv/project"),
                home,
            };
            store.add_checkout(project.id, &checkout).await.unwrap();
        }
        store
            .set_home(project.id, &gpu_box, &path("/srv/project"))
            .await
            .unwrap();
        let checkouts = store.checkouts(project.id).await.unwrap();
        assert_eq!(checkouts.len(), 2);
        let homes: Vec<EndpointId> = checkouts
            .iter()
            .filter(|c| c.home)
            .map(|c| c.machine)
            .collect();
        assert_eq!(homes, vec![gpu_box]);

        let roles = Roles {
            issues: Some(IssueHost::Github),
            prs: IssueHost::from_stored("forgejo:git.example.com"),
        };
        store
            .set_project_setup(project.id, &roles, Visibility::Private, "trunk")
            .await
            .unwrap();
        store.set_pin(project.id, Some(&build_box)).await.unwrap();
        let row = store.project(&user, project.id).await.unwrap().unwrap();
        assert_eq!(row.roles, roles);
        assert_eq!(row.visibility, Visibility::Private);
        assert_eq!(row.default_branch, "trunk");
        assert_eq!(row.pin, Some(build_box));
        store.set_pin(project.id, None).await.unwrap();
        let row = store.project(&user, project.id).await.unwrap().unwrap();
        assert_eq!(row.pin, None);
    }

    #[tokio::test]
    async fn slugs_stay_unique() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let made = store
            .create_tasks(project.id, &[new_task("Fix it"), new_task("Fix it")])
            .await
            .unwrap();
        let slugs: Vec<&str> = made.iter().map(|task| task.slug.as_str()).collect();
        assert_eq!(slugs, ["fix-it", "fix-it-2"]);
        let third = store
            .create_tasks(project.id, &[new_task("Fix it!")])
            .await
            .unwrap();
        assert_eq!(third[0].slug.as_str(), "fix-it-3");
        let found = store
            .task_by_slug(project.id, "fix-it-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, made[1].id);
        assert_eq!(found.state, TaskState::Planned);
    }

    #[tokio::test]
    async fn new_tasks_keep_their_start() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let start = StartRequest {
            machine: Some("gpu-box".into()),
            agent: Some(WorkerAgent::Codex),
            persona: None,
            inputs: Vec::new(),
        };
        let queued = NewTask {
            start: Some(start.clone()),
            ..new_task("Render")
        };
        let made = store
            .create_tasks(project.id, &[queued, new_task("Ship")])
            .await
            .unwrap();
        assert_eq!(made[0].start, Some(start));
        assert_eq!(made[1].start, None);
    }

    #[tokio::test]
    async fn tasks_wait_for_earlier_ones_of_the_same_call() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let second = NewTask {
            depends_on: Some(After::Earlier(0)),
            ..new_task("Render")
        };
        let made = store
            .create_tasks(project.id, &[new_task("Build"), second])
            .await
            .unwrap();
        assert_eq!(made[1].depends_on, Some(made[0].id));
        let later = NewTask {
            depends_on: Some(After::Earlier(1)),
            ..new_task("Ship")
        };
        assert!(store.create_tasks(project.id, &[later]).await.is_err());
        assert_eq!(store.tasks(project.id).await.unwrap().len(), 2);
        store.set_task_after(made[1].id, None).await.unwrap();
        let render = store.task_by_slug(project.id, "render").await.unwrap();
        assert_eq!(render.unwrap().depends_on, None);
    }

    #[tokio::test]
    async fn tasks_keep_their_fields() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let first = store
            .create_tasks(project.id, &[new_task("Build")])
            .await
            .unwrap();
        let second = NewTask {
            title: "Render".into(),
            body: "On the GPU".into(),
            depends_on: Some(After::Task(first[0].id)),
            needs: vec![Capability::Cuda, Capability::Blender],
            start: None,
        };
        let made = store.create_tasks(project.id, &[second]).await.unwrap();
        let id = made[0].id;
        let host = IssueHost::Github;
        let issue = IssueRef {
            number: 12,
            url: "https://example.com/acme/widget/issues/12".into(),
        };
        store.set_task_issue(id, &host, &issue).await.unwrap();
        store
            .set_task_pr(id, "https://example.com/acme/widget/pull/13")
            .await
            .unwrap();
        store.set_pr_seen(id, "abc123").await.unwrap();
        store.set_task_state(id, TaskState::Blocked).await.unwrap();
        store
            .update_task_text(id, Some("Render it"), None)
            .await
            .unwrap();
        let request = StartRequest {
            machine: Some("gpu-box".into()),
            agent: Some(WorkerAgent::Codex),
            persona: None,
            inputs: vec![InputFile {
                machine: None,
                path: "/srv/project/scene.blend".into(),
            }],
        };
        store.set_task_start(id, Some(&request)).await.unwrap();

        let task = store.task(&user, id).await.unwrap().unwrap();
        assert_eq!(task.title, "Render it");
        assert_eq!(task.body, "On the GPU");
        assert_eq!(task.depends_on, Some(first[0].id));
        assert_eq!(task.needs, vec![Capability::Cuda, Capability::Blender]);
        assert_eq!(task.issue_host, Some(host));
        assert_eq!(task.issue, Some(issue));
        assert_eq!(task.pr_seen.as_deref(), Some("abc123"));
        assert_eq!(task.state, TaskState::Blocked);
        assert_eq!(task.start, Some(request));
        assert!(
            store
                .task(&UserKey::for_test("bob"), id)
                .await
                .unwrap()
                .is_none()
        );

        let queued = store.queued_tasks(&user).await.unwrap();
        assert_eq!(queued.iter().map(|t| t.id).collect::<Vec<_>>(), vec![id]);
        store.set_task_start(id, None).await.unwrap();
        assert!(store.queued_tasks(&user).await.unwrap().is_empty());
        assert_eq!(store.tasks(project.id).await.unwrap().len(), 2);
        let counts = store.open_counts(project.id).await.unwrap();
        assert_eq!(
            counts,
            OpenCounts {
                tasks: 2,
                workers: 0
            }
        );
        store.set_task_state(id, TaskState::Dropped).await.unwrap();
        assert_eq!(store.open_counts(project.id).await.unwrap().tasks, 1);
    }

    #[tokio::test]
    async fn caps_refuse_past_three_per_project() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 5).await;
        let (build_box, gpu_box, spare_box) = (machine(), machine(), machine());
        let a = start(&store, &user, &worker_for(tasks[0], build_box, "/srv/a"))
            .await
            .unwrap();
        start(&store, &user, &worker_for(tasks[1], build_box, "/srv/b"))
            .await
            .unwrap();
        start(&store, &user, &worker_for(tasks[2], gpu_box, "/srv/c"))
            .await
            .unwrap();
        let refused = start(&store, &user, &worker_for(tasks[3], spare_box, "/srv/d")).await;
        assert!(matches!(refused, Err(StartRefused::ProjectFull)));
        assert_eq!(store.open_counts(project.id).await.unwrap().workers, 3);

        store
            .set_worker_state(a.id, WorkerState::Ended)
            .await
            .unwrap();
        start(&store, &user, &worker_for(tasks[3], spare_box, "/srv/d"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_refused_start_leaves_no_worker() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 4).await;
        for (n, task) in tasks.iter().take(3).enumerate() {
            start(
                &store,
                &user,
                &worker_for(*task, machine(), &format!("/srv/{n}")),
            )
            .await
            .unwrap();
        }
        let refused = start(&store, &user, &worker_for(tasks[3], machine(), "/srv/3")).await;
        assert!(matches!(refused, Err(StartRefused::ProjectFull)));
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_workers")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(rows, 3);
    }

    #[tokio::test]
    async fn an_ended_worker_stays_ended() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 1).await;
        let worker = start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
            .await
            .unwrap();
        assert!(
            store
                .set_worker_state(worker.id, WorkerState::Running)
                .await
                .unwrap()
        );
        assert!(
            store
                .set_worker_state(worker.id, WorkerState::Ended)
                .await
                .unwrap()
        );
        let ended_at = store
            .worker(&user, worker.id)
            .await
            .unwrap()
            .unwrap()
            .ended_at;
        assert!(ended_at.is_some());
        for live in WorkerState::LIVE {
            assert!(!store.set_worker_state(worker.id, live).await.unwrap());
        }
        let row = store.worker(&user, worker.id).await.unwrap().unwrap();
        assert_eq!(row.state, WorkerState::Ended);
        assert_eq!(row.ended_at, ended_at);
        // Ending it another way still moves it, keeping when it ended.
        assert!(
            store
                .set_worker_state(worker.id, WorkerState::Failed)
                .await
                .unwrap()
        );
        let row = store.worker(&user, worker.id).await.unwrap().unwrap();
        assert_eq!(row.state, WorkerState::Failed);
        assert_eq!(row.ended_at, ended_at);
    }

    #[tokio::test]
    async fn caps_refuse_past_two_per_machine() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let widget = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let gadget = store
            .ensure_project(&user, &repo("acme/gadget"))
            .await
            .unwrap();
        let widget_tasks = tasks_in(&store, widget.id, 2).await;
        let gadget_tasks = tasks_in(&store, gadget.id, 1).await;
        let build_box = machine();
        start(
            &store,
            &user,
            &worker_for(widget_tasks[0], build_box, "/srv/a"),
        )
        .await
        .unwrap();
        start(
            &store,
            &user,
            &worker_for(widget_tasks[1], build_box, "/srv/b"),
        )
        .await
        .unwrap();
        let refused = start(
            &store,
            &user,
            &worker_for(gadget_tasks[0], build_box, "/srv/c"),
        )
        .await;
        assert!(matches!(refused, Err(StartRefused::MachineFull)));
        start(
            &store,
            &user,
            &worker_for(gadget_tasks[0], machine(), "/srv/c"),
        )
        .await
        .unwrap();
        assert_eq!(
            store
                .live_workers_on(&user, &build_box)
                .await
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn caps_refuse_past_six_per_user() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let mut tasks = Vec::new();
        for name in ["acme/one", "acme/two", "acme/three"] {
            let project = store.ensure_project(&user, &repo(name)).await.unwrap();
            tasks.extend(tasks_in(&store, project.id, 2).await);
        }
        let last = store
            .ensure_project(&user, &repo("acme/four"))
            .await
            .unwrap();
        tasks.extend(tasks_in(&store, last.id, 1).await);
        for (n, task) in tasks.iter().take(6).enumerate() {
            start(
                &store,
                &user,
                &worker_for(*task, machine(), &format!("/srv/{n}")),
            )
            .await
            .unwrap();
        }
        let refused = start(&store, &user, &worker_for(tasks[6], machine(), "/srv/6")).await;
        assert!(matches!(refused, Err(StartRefused::UserFull)));
        assert_eq!(store.live_workers(&user).await.unwrap().len(), 6);
        // Another user's slots are their own.
        let bob = UserKey::for_test("bob");
        let theirs = store.ensure_project(&bob, &repo("acme/one")).await.unwrap();
        let bob_task = tasks_in(&store, theirs.id, 1).await;
        start(&store, &bob, &worker_for(bob_task[0], machine(), "/srv/0"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn one_folder_one_writer() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 2).await;
        let build_box = machine();
        let first = start(
            &store,
            &user,
            &worker_for(tasks[0], build_box, "/srv/project"),
        )
        .await
        .unwrap();
        let refused = start(
            &store,
            &user,
            &worker_for(tasks[1], build_box, "/srv/project"),
        )
        .await;
        assert!(matches!(refused, Err(StartRefused::FolderTaken)));
        // The same folder on another machine is free.
        start(
            &store,
            &user,
            &worker_for(tasks[1], machine(), "/srv/project"),
        )
        .await
        .unwrap();
        // And on the same machine once the first worker failed.
        store
            .set_worker_state(first.id, WorkerState::Failed)
            .await
            .unwrap();
        let row = store.worker(&user, first.id).await.unwrap().unwrap();
        assert!(row.ended_at.is_some());
        let third = tasks_in(&store, project.id, 1).await;
        start(
            &store,
            &user,
            &worker_for(third[0], build_box, "/srv/project"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn one_worker_per_task() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 1).await;
        start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
            .await
            .unwrap();
        let refused = start(&store, &user, &worker_for(tasks[0], machine(), "/srv/b")).await;
        assert!(matches!(refused, Err(StartRefused::TaskHasWorker)));
    }

    #[tokio::test]
    async fn a_task_of_another_user_does_not_start() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 1).await;
        let bob = UserKey::for_test("bob");
        assert!(
            store
                .start_worker(&bob, &worker_for(tasks[0], machine(), "/srv/a"))
                .await
                .is_err()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_starts_take_one_slot() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 4).await;
        start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
            .await
            .unwrap();
        start(&store, &user, &worker_for(tasks[1], machine(), "/srv/b"))
            .await
            .unwrap();
        // Open two connections first, so both starts get one at once and
        // their transactions overlap instead of one waiting on a connect.
        // The test runs multi-threaded so the pool can take them back in
        // between; without the advisory lock both starts then win.
        let (one, two) = tokio::join!(store.pool().acquire(), store.pool().acquire());
        drop((one.unwrap(), two.unwrap()));
        // Both race for the last slot in the project, on separate connections.
        let first = worker_for(tasks[2], machine(), "/srv/c");
        let second = worker_for(tasks[3], machine(), "/srv/d");
        let (one, two) = tokio::join!(
            store.start_worker(&user, &first),
            store.start_worker(&user, &second)
        );
        let results = [one.unwrap(), two.unwrap()];
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(StartRefused::ProjectFull)))
                .count(),
            1
        );
        assert_eq!(store.open_counts(project.id).await.unwrap().workers, 3);
    }

    #[tokio::test]
    async fn workers_list_ended_from_the_last_day() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 3).await;
        let build_box = machine();
        let old = start(&store, &user, &worker_for(tasks[0], build_box, "/srv/a"))
            .await
            .unwrap();
        let recent = start(&store, &user, &worker_for(tasks[1], build_box, "/srv/b"))
            .await
            .unwrap();
        store
            .set_worker_state(old.id, WorkerState::Ended)
            .await
            .unwrap();
        store
            .set_worker_state(recent.id, WorkerState::Ended)
            .await
            .unwrap();
        sqlx::query("UPDATE project_workers SET ended_at = now() - interval '2 days' WHERE id=$1")
            .bind(old.id.0)
            .execute(store.pool())
            .await
            .unwrap();
        let live = start(&store, &user, &worker_for(tasks[2], build_box, "/srv/c"))
            .await
            .unwrap();
        store
            .set_worker_session(live.id, "session-1")
            .await
            .unwrap();
        let at = Utc::now();
        store
            .worker_turn(live.id, 4, "Done with step one", at)
            .await
            .unwrap();

        let since = Utc::now() - TimeDelta::days(1);
        let listed: Vec<WorkerId> = store
            .workers(project.id, since)
            .await
            .unwrap()
            .iter()
            .map(|w| w.id)
            .collect();
        assert_eq!(listed, vec![recent.id, live.id]);
        let row = store.worker(&user, live.id).await.unwrap().unwrap();
        assert_eq!(row.state, WorkerState::Waiting);
        assert_eq!(row.session.as_deref(), Some("session-1"));
        assert_eq!(row.turns_seen, 4);
        assert_eq!(row.last_output.as_deref(), Some("Done with step one"));
        assert!(row.last_turn_at.is_some());
        assert!(!row.asking);
        store.set_worker_asking(live.id, true).await.unwrap();
        assert!(store.worker(&user, live.id).await.unwrap().unwrap().asking);
        assert!(
            store
                .worker(&UserKey::for_test("bob"), live.id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn starting_workers_fail_on_startup() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let project = store
            .ensure_project(&user, &repo("acme/widget"))
            .await
            .unwrap();
        let tasks = tasks_in(&store, project.id, 2).await;
        let starting = start(&store, &user, &worker_for(tasks[0], machine(), "/srv/a"))
            .await
            .unwrap();
        let running = start(&store, &user, &worker_for(tasks[1], machine(), "/srv/b"))
            .await
            .unwrap();
        store
            .set_worker_state(running.id, WorkerState::Running)
            .await
            .unwrap();
        let failed = store.fail_starting_workers().await.unwrap();
        assert_eq!(
            failed.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![starting.id]
        );
        assert_eq!(failed[0].state, WorkerState::Failed);
        assert!(failed[0].ended_at.is_some());
        let row = store.worker(&user, running.id).await.unwrap().unwrap();
        assert_eq!(row.state, WorkerState::Running);
        assert_eq!(store.live_workers(&user).await.unwrap().len(), 1);
    }
}
