//! The workers running a project's tasks, capped per machine and per user.

use super::{super::Store, WORKER_COLUMNS, live_states, worker_from};
use crate::{
    account::user::UserKey,
    projects::model::{ProjectId, TaskState, WorkerId, WorkerState},
    store::WorkerRow,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

impl Store {
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
