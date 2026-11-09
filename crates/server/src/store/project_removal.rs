//! Forgetting a project or one of its machines; only rows go, and names match without case.

use super::{
    Bumped, Store,
    chats::delete_chat,
    projects::{WORKER_COLUMNS, live_states, worker_from},
    terminals::{self, TerminalRow},
};
use crate::{account::user::UserKey, projects::model::ProjectId, store::WorkerRow};
use anyhow::Result;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// What removing projects or a machine left to do on the machines once committed.
#[derive(Debug, Default)]
pub(crate) struct RemovedProject {
    /// The `projects` rows that went, by id.
    pub(crate) projects: Vec<ProjectId>,
    /// Workers that were live, whose sessions are to be closed.
    pub(crate) workers: Vec<WorkerRow>,
    /// Its terminals, whose commands are to be stopped.
    pub(crate) terminals: Vec<TerminalRow>,
    /// Branch review sessions still reading, to be closed.
    pub(crate) reviewers: Vec<ReviewSession>,
    /// Some machine synced it.
    pub(crate) synced: bool,
}

/// What a removal's transaction took with it, for after it commits.
#[derive(Debug, Default)]
pub(super) struct Gone {
    pub(super) removed: RemovedProject,
    /// Files whose bytes are to be deleted.
    pub(super) files: Vec<String>,
    /// Update logs to announce.
    pub(super) bumped: Vec<Bumped>,
}

/// A branch reviewer's session on a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSession {
    /// The node's key.
    pub(crate) endpoint: String,
    pub(crate) agent: String,
    pub(crate) session: String,
}

/// The body of [`Store::remove_project`], in its transaction; false when nothing of that name exists.
pub(super) async fn remove_project_rows(
    tx: &mut PgConnection,
    user: &UserKey,
    name: &str,
    gone: &mut Gone,
) -> Result<bool> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM projects WHERE user_id=$1 AND lower(name)=lower($2)
         ORDER BY id FOR UPDATE",
    )
    .bind(user.as_ref())
    .bind(name)
    .fetch_all(&mut *tx)
    .await?;
    let workers = sqlx::query(&format!(
        "SELECT {WORKER_COLUMNS} FROM project_workers w
         WHERE w.project_id = ANY($1) AND w.state = ANY($2) ORDER BY w.started_at"
    ))
    .bind(&ids)
    .bind(live_states())
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(worker_from)
    .collect::<Result<Vec<_>>>()?;
    let chats: Vec<Uuid> = sqlx::query_scalar(
        "SELECT chat_id FROM project_agents WHERE project_id = ANY($1)
         UNION SELECT chat_id FROM project_thread_chats WHERE project_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;

    for chat in chats {
        let deleted = delete_chat(&mut *tx, chat).await?;
        gone.files.extend(deleted.files);
        gone.bumped.extend(deleted.bumped);
    }

    sqlx::query("DELETE FROM projects WHERE id = ANY($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    let imports =
        sqlx::query("DELETE FROM project_imports WHERE user_id=$1 AND lower(project)=lower($2)")
            .bind(user.as_ref())
            .bind(name)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    let synced =
        sqlx::query("DELETE FROM project_sync WHERE user_id=$1 AND lower(project)=lower($2)")
            .bind(user.as_ref())
            .bind(name)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    for table in ["pinned_projects", "projects_on_top", "paused_syncs"] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE user_id=$1 AND lower(project)=lower($2)"
        ))
        .bind(user.as_ref())
        .bind(name)
        .execute(&mut *tx)
        .await?;
    }

    let terminals = sqlx::query(&format!(
        "DELETE FROM terminals WHERE user_id=$1 AND lower(project)=lower($2)
         RETURNING {}",
        terminals::COLUMNS
    ))
    .bind(user.as_ref())
    .bind(name)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(terminals::row_from)
    .collect::<Result<Vec<_>>>()?;
    let reviewers = sqlx::query(
        "SELECT r.endpoint_id, s.agent, s.session FROM branch_review_shards s
         JOIN branch_reviews r ON r.id = s.review_id
         WHERE r.user_id=$1 AND lower(r.project)=lower($2) AND s.state = 'reading'
         ORDER BY r.id, s.shard",
    )
    .bind(user.as_ref())
    .bind(name)
    .fetch_all(&mut *tx)
    .await?
    .iter()
    .map(|row| {
        Ok(ReviewSession {
            endpoint: row.try_get(0)?,
            agent: row.try_get(1)?,
            session: row.try_get(2)?,
        })
    })
    .collect::<Result<Vec<_>>>()?;
    let reviews =
        sqlx::query("DELETE FROM branch_reviews WHERE user_id=$1 AND lower(project)=lower($2)")
            .bind(user.as_ref())
            .bind(name)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if ids.is_empty() && imports == 0 && synced == 0 && terminals.is_empty() && reviews == 0 {
        return Ok(false);
    }

    let removed = &mut gone.removed;
    removed.projects.extend(ids.into_iter().map(ProjectId));
    removed.workers.extend(workers);
    removed.terminals.extend(terminals);
    removed.reviewers.extend(reviewers);
    removed.synced |= synced > 0;
    Ok(true)
}

impl Store {
    /// Deletes every row of project `name` and its chats; `None` when nothing of that name exists.
    pub(crate) async fn remove_project(
        &self,
        user: &UserKey,
        name: &str,
    ) -> Result<Option<RemovedProject>> {
        let mut tx = self.pool.begin().await?;
        let mut gone = Gone::default();

        if !remove_project_rows(&mut tx, user, name, &mut gone).await? {
            return Ok(None);
        }

        tx.commit().await?;
        self.announce(&gone.bumped);
        self.blobs.delete(&gone.files).await?;
        Ok(Some(gone.removed))
    }
}

#[cfg(test)]
#[path = "../tests/store_project_removal.rs"]
mod tests;
