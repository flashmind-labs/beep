//! Projects, their checkouts, agent chats, tasks and workers.

mod rows;
pub(super) use rows::worker_from;
pub(crate) use rows::{
    After, CheckoutRow, IssueAuthor, NewTask, NewWorker, OpenCounts, OverviewRow, ProjectRow,
    StartRefused, TaskRow, TrackedIssue, WorkerRow,
};
use rows::{checkout_from, project_from, task_from};

mod overview;

mod tasks;
mod workers;

use super::{
    Store,
    chats::{SessionChat, ensure_session},
};
use crate::{
    account::user::UserKey,
    projects::model::{
        IssueHost, MachinePath, ProjectId, RepoName, Roles, Visibility, WorkerState,
    },
};
use anyhow::{Result, bail};
use iroh::EndpointId;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

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
}

#[cfg(test)]
mod tests;
