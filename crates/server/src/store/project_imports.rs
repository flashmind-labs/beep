//! The projects a user imported onto their machines (see
//! `crate::imports`).

use super::Store;
use crate::{projects::model::RepoName, user::UserKey};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row};
use std::collections::HashMap;
use uuid::Uuid;

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
    /// The user's fork cloned as `origin`, `None` for the repository itself.
    pub(crate) fork: Option<RepoName>,
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
    /// the same machine to `folder`, and makes the project if it is new.
    /// Gives the row as stored.
    pub(crate) async fn save_import(
        &self,
        user: &UserKey,
        import: NewImport<'_>,
    ) -> Result<ImportRow> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO project_imports (user_id, project, endpoint_id, folder, url)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, project, endpoint_id)
             DO UPDATE SET folder=excluded.folder, url=excluded.url
             RETURNING project, endpoint_id, folder, url, imported_at, fork",
        )
        .bind(user.as_str())
        .bind(import.project)
        .bind(import.endpoint)
        .bind(import.folder)
        .bind(import.url)
        .fetch_one(&mut *tx)
        .await?;
        let row = import_row(&row)?;
        ensure_imported_project(&mut tx, user, import.project).await?;
        tx.commit().await?;
        Ok(row)
    }

    /// The user's imports, by project, then machine.
    pub(crate) async fn imports(&self, user: &UserKey) -> Result<Vec<ImportRow>> {
        let rows = sqlx::query(
            "SELECT project, endpoint_id, folder, url, imported_at, fork FROM project_imports
             WHERE user_id=$1 ORDER BY project, endpoint_id",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(import_row).collect()
    }

    /// Notes the user's fork the import of `project` on `endpoint` cloned
    /// or linked, or that it is the repository itself (`None`).
    pub(crate) async fn set_import_fork(
        &self,
        user: &UserKey,
        project: &str,
        endpoint: &str,
        fork: Option<&RepoName>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE project_imports SET fork=$4
             WHERE user_id=$1 AND project=$2 AND endpoint_id=$3",
        )
        .bind(user.as_str())
        .bind(project)
        .bind(endpoint)
        .bind(fork.map(ToString::to_string))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records a checkout found on a machine as the project's folder there,
    /// unless the project already has one on that machine. Says whether it
    /// was recorded. Makes the project if it is new.
    pub(crate) async fn link_import(&self, user: &UserKey, import: NewImport<'_>) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
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
        .execute(&mut *tx)
        .await?;
        ensure_imported_project(&mut tx, user, import.project).await?;
        tx.commit().await?;
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

/// Makes the project an import names, unless the user has one of that
/// name in any case, so its agent and board exist without its chat being
/// opened first. A name that is not `owner/repo` gets no project.
async fn ensure_imported_project(
    conn: &mut PgConnection,
    user: &UserKey,
    project: &str,
) -> Result<()> {
    let Ok(name) = RepoName::parse(project) else {
        log::warn!("imported project has no project row, not owner/repo: {project}");
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO projects (id, user_id, name)
         SELECT $1, $2, $3
         WHERE NOT EXISTS (SELECT 1 FROM projects WHERE user_id=$2 AND lower(name)=lower($3))
         ON CONFLICT (user_id, name) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(user.as_str())
    .bind(name.to_string())
    .execute(conn)
    .await?;
    Ok(())
}

fn import_row(row: &sqlx::postgres::PgRow) -> Result<ImportRow> {
    Ok(ImportRow {
        project: row.try_get(0)?,
        endpoint: row.try_get(1)?,
        folder: row.try_get(2)?,
        url: row.try_get(3)?,
        imported_at: row.try_get(4)?,
        fork: row
            .try_get::<Option<String>, _>(5)?
            .and_then(|fork| RepoName::parse(&fork).ok()),
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

    async fn project_names(store: &Store, user: &UserKey) -> Vec<String> {
        store
            .projects(user)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.name.to_string())
            .collect()
    }

    #[tokio::test]
    async fn an_imported_project_is_listed_once_and_stays_removed() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        store
            .save_import(&alice, import("org/a", "node-1", "/w/projects/org/a"))
            .await
            .unwrap();
        store
            .link_import(&alice, import("org/a", "node-2", "/srv/a"))
            .await
            .unwrap();
        store
            .link_import(&alice, import("org/b", "node-2", "/srv/b"))
            .await
            .unwrap();
        store
            .save_import(&alice, import("Org/A", "node-3", "/w/projects/Org/A"))
            .await
            .unwrap();
        assert_eq!(project_names(&store, &alice).await, ["org/a", "org/b"]);
        assert!(
            project_names(&store, &UserKey::for_test("bob"))
                .await
                .is_empty()
        );
        store.remove_project(&alice, "org/a").await.unwrap();
        assert_eq!(project_names(&store, &alice).await, ["org/b"]);
    }

    #[tokio::test]
    async fn imports_from_before_get_their_project() {
        let db = TestDb::new().await.unwrap();
        let pool = db.pool_at(74).await.unwrap();
        for (user, project, endpoint) in [
            ("alice", "org/a", "node-1"),
            ("alice", "org/a", "node-2"),
            ("alice", "ORG/A", "node-3"),
            ("alice", "org/opened", "node-1"),
            ("alice", "not a repo", "node-1"),
            ("bob", "org/a", "node-1"),
        ] {
            sqlx::query(
                "INSERT INTO project_imports (user_id, project, endpoint_id, folder, url)
                 VALUES ($1, $2, $3, '/w', 'https://github.com/org/a')",
            )
            .bind(user)
            .bind(project)
            .bind(endpoint)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO projects (id, user_id, name) VALUES ($1, 'alice', 'Org/Opened')")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let names = project_names(&store, &alice).await;
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&"Org/Opened".to_owned()), "{names:?}");
        assert!(
            names.iter().any(|n| n.eq_ignore_ascii_case("org/a")),
            "{names:?}"
        );
        assert_eq!(
            project_names(&store, &UserKey::for_test("bob")).await,
            ["org/a"]
        );
    }
}
