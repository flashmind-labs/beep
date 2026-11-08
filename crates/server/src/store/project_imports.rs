//! Projects a user imported onto their machines (see `crate::projects::imports`).

use super::Store;
use crate::account::user::UserKey;
use anyhow::Result;
use beep_projects::imports::records::{ImportRow, NewImport};
use beep_projects::model::RepoName;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

impl Store {
    /// Records an import (or moves it to `folder`), making the project if new; returns the stored row.
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
        .bind(user.as_ref())
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
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(import_row).collect()
    }

    /// Notes the fork the import of `project` on `endpoint` uses, or `None` for the repository itself.
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
        .bind(user.as_ref())
        .bind(project)
        .bind(endpoint)
        .bind(fork.map(ToString::to_string))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records a found checkout as the project's folder on that machine unless it has one; makes the project if new.
    pub(crate) async fn link_import(&self, user: &UserKey, import: NewImport<'_>) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let done = sqlx::query(
            "INSERT INTO project_imports (user_id, project, endpoint_id, folder, url)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, project, endpoint_id) DO NOTHING",
        )
        .bind(user.as_ref())
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
}

/// Makes the project an import names unless one exists (any case); non-`owner/repo` names get none.
async fn ensure_imported_project(
    conn: &mut PgConnection,
    user: &UserKey,
    project: &str,
) -> Result<()> {
    let Ok(name) = project.parse::<RepoName>() else {
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
    .bind(user.as_ref())
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
            .and_then(|fork| fork.parse::<RepoName>().ok()),
    })
}

#[cfg(test)]
#[path = "../tests/store_project_imports.rs"]
mod tests;
