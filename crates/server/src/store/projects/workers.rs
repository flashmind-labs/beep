//! The workers running a project's tasks, capped per machine and per user.

use super::{super::Store, WORKER_COLUMNS, end, live_states, worker_from};
use crate::{
    account::user::UserKey,
    projects::model::{
        MAX_MACHINE_WORKERS, MAX_PROJECT_WORKERS, MAX_USER_WORKERS, ProjectId, TaskState, WorkerId,
        WorkerState,
    },
    store::{NewWorker, StartRefused, WorkerRow},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// The refusal a unique violation on one of the worker locks stands for.
pub(super) fn lock_refusal(error: &sqlx::Error) -> Option<StartRefused> {
    let sqlx::Error::Database(error) = error else {
        return None;
    };
    match error.constraint()? {
        "project_workers_folder" => Some(StartRefused::FolderTaken),
        "project_workers_task" => Some(StartRefused::TaskHasWorker),
        _ => None,
    }
}

/// The body of [`Store::start_worker`], in its transaction.
pub(super) async fn start_in(
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
    /// Starts a worker if caps and locks allow; serialized per user so two cannot take the last slot.
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

    /// Moves the worker to `state`, noting the end; ended or failed workers never revive. Says whether it moved.
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

    /// Notes a finished turn (worker waits, `turns` heard); ignored for ended workers or stale turns. Says whether noted.
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

    /// The project's live workers plus those ended since `since`, oldest first.
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

    /// Fails every still-starting worker at startup, since the server starting them is gone.
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

    pub(crate) async fn project_owner(&self, project: ProjectId) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT user_id FROM projects WHERE id=$1")
                .bind(project.0)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

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
}
