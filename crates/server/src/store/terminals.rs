//! A project's terminals: which background command on which node.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// One terminal as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalRow {
    pub(crate) id: Uuid,
    pub(crate) project: String,
    /// The node's key.
    pub(crate) endpoint: String,
    /// The node's number for the task, or for the terminal when `pty`.
    pub(crate) task: u32,
    /// An interactive terminal (`terminal/1`) rather than a background
    /// task (`process/1`).
    pub(crate) pty: bool,
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) started_at: DateTime<Utc>,
}

pub(super) const COLUMNS: &str =
    "id, project, endpoint_id, task_id, name, command, cwd, started_at, pty";

pub(super) fn row_from(row: &PgRow) -> Result<TerminalRow> {
    let task: i64 = row.try_get(3)?;
    Ok(TerminalRow {
        id: row.try_get(0)?,
        project: row.try_get(1)?,
        endpoint: row.try_get(2)?,
        task: u32::try_from(task)?,
        name: row.try_get(4)?,
        command: row.try_get(5)?,
        cwd: row.try_get(6)?,
        started_at: row.try_get(7)?,
        pty: row.try_get(8)?,
    })
}

impl Store {
    pub(crate) async fn add_terminal(&self, user: &UserKey, row: &TerminalRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO terminals (id, user_id, project, endpoint_id, task_id, name, command, cwd, started_at, pty)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(row.id)
        .bind(user.as_str())
        .bind(&row.project)
        .bind(&row.endpoint)
        .bind(i64::from(row.task))
        .bind(&row.name)
        .bind(&row.command)
        .bind(&row.cwd)
        .bind(row.started_at)
        .bind(row.pty)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `project`'s terminals, oldest first.
    pub(crate) async fn terminals(
        &self,
        user: &UserKey,
        project: &str,
    ) -> Result<Vec<TerminalRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM terminals WHERE user_id=$1 AND project=$2 ORDER BY started_at, id"
        ))
        .bind(user.as_str())
        .bind(project)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_from).collect()
    }

    pub(crate) async fn terminal(&self, user: &UserKey, id: Uuid) -> Result<Option<TerminalRow>> {
        let row = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM terminals WHERE user_id=$1 AND id=$2"
        ))
        .bind(user.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_from).transpose()
    }

    /// Forgets a terminal; false when the user had no such one.
    pub(crate) async fn remove_terminal(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let done = sqlx::query("DELETE FROM terminals WHERE user_id=$1 AND id=$2")
            .bind(user.as_str())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;

    fn terminal(project: &str, task: u32) -> TerminalRow {
        TerminalRow {
            id: Uuid::new_v4(),
            project: project.into(),
            endpoint: "node".into(),
            task,
            pty: false,
            name: "dev-server".into(),
            command: "npm run dev".into(),
            cwd: "/srv/project".into(),
            started_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn terminals_are_kept_per_user_and_project() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let one = terminal("acme/widget", 3);
        store.add_terminal(&user, &one).await.unwrap();
        store
            .add_terminal(&user, &terminal("acme/relay", 4))
            .await
            .unwrap();
        let listed = store.terminals(&user, "acme/widget").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].task, 3);
        assert_eq!(listed[0].command, "npm run dev");
        assert!(!listed[0].pty);
        let shell = TerminalRow {
            pty: true,
            ..terminal("acme/widget", 7)
        };
        store.add_terminal(&user, &shell).await.unwrap();
        let read = store.terminal(&user, shell.id).await.unwrap();
        assert_eq!(read.map(|row| (row.task, row.pty)), Some((7, true)));
        assert!(store.remove_terminal(&user, shell.id).await.unwrap());
        assert!(store.terminal(&bob, one.id).await.unwrap().is_none());
        assert!(!store.remove_terminal(&bob, one.id).await.unwrap());
        assert!(store.remove_terminal(&user, one.id).await.unwrap());
        assert!(
            store
                .terminals(&user, "acme/widget")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
