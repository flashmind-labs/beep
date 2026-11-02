//! The projects a user imported onto their machines (see
//! `crate::imports`).

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;
use std::collections::HashMap;

/// One project imported onto one machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRow {
    /// `org/repo`.
    pub project: String,
    /// The node's key, as the store keeps it.
    pub endpoint: String,
    pub folder: String,
    /// The repository's web page.
    pub url: String,
    pub imported_at: DateTime<Utc>,
}

/// An import to record.
pub struct NewImport<'a> {
    pub project: &'a str,
    pub endpoint: &'a str,
    pub folder: &'a str,
    pub url: &'a str,
}

impl Store {
    /// Records an import, or moves an earlier one of the same project on
    /// the same machine to `folder`. Gives the row as stored.
    pub(crate) async fn save_import(
        &self,
        user: &UserKey,
        import: NewImport<'_>,
    ) -> Result<ImportRow> {
        let row = sqlx::query(
            "INSERT INTO project_imports (user_id, project, endpoint_id, folder, url)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, project, endpoint_id)
             DO UPDATE SET folder=excluded.folder, url=excluded.url
             RETURNING project, endpoint_id, folder, url, imported_at",
        )
        .bind(user.as_str())
        .bind(import.project)
        .bind(import.endpoint)
        .bind(import.folder)
        .bind(import.url)
        .fetch_one(&self.pool)
        .await?;
        import_row(&row)
    }

    /// The user's imports, by project, then machine.
    pub(crate) async fn imports(&self, user: &UserKey) -> Result<Vec<ImportRow>> {
        let rows = sqlx::query(
            "SELECT project, endpoint_id, folder, url, imported_at FROM project_imports
             WHERE user_id=$1 ORDER BY project, endpoint_id",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(import_row).collect()
    }

    /// Records a checkout found on a machine as the project's folder there,
    /// unless the project already has one on that machine. Says whether it
    /// was recorded.
    pub(crate) async fn link_import(&self, user: &UserKey, import: NewImport<'_>) -> Result<bool> {
        let done = sqlx::query(
            "INSERT INTO project_imports (user_id, project, endpoint_id, folder, url)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, project, endpoint_id) DO NOTHING",
        )
        .bind(user.as_str())
        .bind(import.project)
        .bind(import.endpoint)
        .bind(import.folder)
        .bind(import.url)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    /// Notes that the user's machine `endpoint` was looked through for
    /// checkouts at `at`.
    pub(crate) async fn save_checkout_check(
        &self,
        user: &UserKey,
        endpoint: &str,
        at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO checkout_checks (user_id, endpoint_id, checked_at) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, endpoint_id) DO UPDATE SET checked_at=excluded.checked_at",
        )
        .bind(user.as_str())
        .bind(endpoint)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// When each of the user's machines was last looked through, by node key.
    pub(crate) async fn checkout_checks(
        &self,
        user: &UserKey,
    ) -> Result<HashMap<String, DateTime<Utc>>> {
        let rows =
            sqlx::query("SELECT endpoint_id, checked_at FROM checkout_checks WHERE user_id=$1")
                .bind(user.as_str())
                .fetch_all(&self.pool)
                .await?;
        rows.iter()
            .map(|row| Ok((row.try_get(0)?, row.try_get(1)?)))
            .collect()
    }
}

fn import_row(row: &sqlx::postgres::PgRow) -> Result<ImportRow> {
    Ok(ImportRow {
        project: row.try_get(0)?,
        endpoint: row.try_get(1)?,
        folder: row.try_get(2)?,
        url: row.try_get(3)?,
        imported_at: row.try_get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;

    fn import<'a>(project: &'a str, endpoint: &'a str, folder: &'a str) -> NewImport<'a> {
        NewImport {
            project,
            endpoint,
            folder,
            url: "https://github.com/org/repo",
        }
    }

    #[tokio::test]
    async fn imports_list_per_user_and_a_second_import_moves_the_folder() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        store
            .save_import(&alice, import("org/b", "node-1", "/w/projects/org/b"))
            .await
            .unwrap();
        store
            .save_import(&alice, import("org/a", "node-1", "/w/projects/org/a"))
            .await
            .unwrap();
        let moved = store
            .save_import(&alice, import("org/a", "node-1", "/w/elsewhere/a"))
            .await
            .unwrap();
        assert_eq!(moved.folder, "/w/elsewhere/a");
        let listed = store.imports(&alice).await.unwrap();
        let names: Vec<_> = listed.iter().map(|r| r.project.as_str()).collect();
        assert_eq!(names, ["org/a", "org/b"]);
        assert_eq!(listed[0].folder, "/w/elsewhere/a");
        assert!(store.imports(&bob).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_found_checkout_never_moves_a_folder_and_checks_keep_the_latest() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        store
            .save_import(&alice, import("org/a", "node-1", "/w/projects/org/a"))
            .await
            .unwrap();
        let linked = store
            .link_import(&alice, import("org/a", "node-2", "/srv/a"))
            .await
            .unwrap();
        assert!(linked);
        let again = store
            .link_import(&alice, import("org/a", "node-1", "/elsewhere/a"))
            .await
            .unwrap();
        assert!(!again);
        let folders: Vec<_> = store
            .imports(&alice)
            .await
            .unwrap()
            .into_iter()
            .map(|row| (row.endpoint, row.folder))
            .collect();
        assert_eq!(
            folders,
            [
                ("node-1".to_owned(), "/w/projects/org/a".to_owned()),
                ("node-2".to_owned(), "/srv/a".to_owned())
            ]
        );
        let first = Utc::now() - chrono::Duration::minutes(5);
        let later = Utc::now();
        store
            .save_checkout_check(&alice, "node-2", first)
            .await
            .unwrap();
        store
            .save_checkout_check(&alice, "node-2", later)
            .await
            .unwrap();
        let checks = store.checkout_checks(&alice).await.unwrap();
        assert_eq!(checks.len(), 1);
        let at = checks["node-2"];
        assert!((at - later).num_milliseconds().abs() < 1, "{at} {later}");
        assert!(
            store
                .checkout_checks(&UserKey::for_test("bob"))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
