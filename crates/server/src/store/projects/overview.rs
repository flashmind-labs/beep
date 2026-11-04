//! The counts and rows behind the projects overview.

use super::{super::Store, OpenCounts, OverviewRow, TASK_COLUMNS, live_states, task_from};
use crate::{
    account::user::UserKey,
    projects::model::{ProjectId, TaskState},
    protocol::TaskCounts,
};
use anyhow::Result;
use sqlx::Row;
use std::collections::HashMap;

impl Store {
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
