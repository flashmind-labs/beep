//! A project's tasks: creating, tracking and moving them through their states.

use super::{super::Store, MAX_SLUG_SUFFIX, TASK_COLUMNS, end, task_from};
use crate::{
    account::user::UserKey,
    projects::model::{IssueHost, IssueRef, ProjectId, StartRequest, TaskId, TaskSlug, TaskState},
    store::{After, IssueAuthor, NewTask, TaskRow, TrackedIssue},
};
use anyhow::{Context, Result, bail};
use sqlx::PgConnection;
use std::collections::HashSet;
use uuid::Uuid;

/// The body of [`Store::create_tasks`], in its transaction.
pub(super) async fn tasks_in(
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

/// The body of [`Store::track_issue`], in its transaction.
pub(super) async fn track_in(
    tx: &mut PgConnection,
    project: ProjectId,
    task: &NewTask,
    host: &IssueHost,
    issue: &IssueRef,
    author: IssueAuthor,
) -> Result<TrackedIssue> {
    // Locking the project makes a second caller wait and find this task.
    sqlx::query("SELECT 1 FROM projects WHERE id=$1 FOR UPDATE")
        .bind(project.0)
        .fetch_optional(&mut *tx)
        .await?
        .context("no such project")?;
    let number = i64::try_from(issue.number)?;
    let tracked = sqlx::query(&format!(
        "SELECT {TASK_COLUMNS} FROM project_tasks t
         WHERE t.project_id=$1 AND t.issue_host=$2 AND t.issue_number=$3
         ORDER BY t.created_at LIMIT 1"
    ))
    .bind(project.0)
    .bind(host.stored())
    .bind(number)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = tracked {
        return Ok(TrackedIssue::Already(task_from(&row)?));
    }
    let made = tasks_in(tx, project, std::slice::from_ref(task)).await?;
    let made = made.first().context("no task was made")?;
    let row = sqlx::query(&format!(
        "UPDATE project_tasks AS t SET issue_host=$2, issue_number=$3, issue_url=$4,
             issue_foreign=$5
         WHERE t.id=$1 RETURNING {TASK_COLUMNS}"
    ))
    .bind(made.id.0)
    .bind(host.stored())
    .bind(number)
    .bind(&issue.url)
    .bind(author == IssueAuthor::Other)
    .fetch_one(&mut *tx)
    .await?;
    Ok(TrackedIssue::Made(task_from(&row)?))
}

impl Store {
    /// Creates `tasks` in `project` in order; clashing slugs get `-2`, `-3` and so on.
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

    /// Moves the task from `from` to `to` unless it moved since read; says whether it did.
    pub(crate) async fn move_task_state(
        &self,
        id: TaskId,
        from: TaskState,
        to: TaskState,
    ) -> Result<bool> {
        let moved = sqlx::query(
            "UPDATE project_tasks SET state=$3, updated_at=now() WHERE id=$1 AND state=$2",
        )
        .bind(id.0)
        .bind(<&str>::from(from))
        .bind(<&str>::from(to))
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(moved == 1)
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

    /// Makes `task` tracking existing `issue` on `host` unless already tracked; all or nothing.
    pub(crate) async fn track_issue(
        &self,
        project: ProjectId,
        task: &NewTask,
        host: &IssueHost,
        issue: &IssueRef,
        author: IssueAuthor,
    ) -> Result<TrackedIssue> {
        let mut tx = self.pool.begin().await?;
        let done = track_in(&mut tx, project, task, host, issue, author).await;
        let keep = done.is_ok();
        end(tx, keep, done).await
    }

    /// Notes Beep commented on the task's issue (only the first time with `once`); says whether noted.
    pub(crate) async fn note_issue_comment(&self, id: TaskId, once: bool) -> Result<bool> {
        let noted = sqlx::query(
            "UPDATE project_tasks SET issue_commented=true, updated_at=now()
             WHERE id=$1 AND NOT ($2 AND issue_commented)",
        )
        .bind(id.0)
        .bind(once)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(noted == 1)
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
}
