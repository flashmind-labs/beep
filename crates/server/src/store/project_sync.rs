//! Which machines sync which projects, and pinned projects.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use sqlx::Row;

/// One machine that syncs a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRow {
    pub project: String,
    /// The node's key, as the store keeps it.
    pub endpoint: String,
    pub folder: String,
}

impl Store {
    pub(crate) async fn project_syncs(&self, user: &UserKey) -> Result<Vec<SyncRow>> {
        let rows = sqlx::query(
            "SELECT project, endpoint_id, folder FROM project_sync WHERE user_id=$1
             ORDER BY project, endpoint_id",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(SyncRow {
                    project: row.try_get(0)?,
                    endpoint: row.try_get(1)?,
                    folder: row.try_get(2)?,
                })
            })
            .collect()
    }

    /// Syncs `row.project` on its machine, in its folder.
    pub(crate) async fn set_project_sync(&self, user: &UserKey, row: &SyncRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO project_sync (user_id, project, endpoint_id, folder)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, project, endpoint_id)
             DO UPDATE SET folder=excluded.folder, updated_at=now()",
        )
        .bind(user.as_str())
        .bind(&row.project)
        .bind(&row.endpoint)
        .bind(&row.folder)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Stops syncing `project` on `endpoint`; `false` when it did not.
    pub(crate) async fn remove_project_sync(
        &self,
        user: &UserKey,
        project: &str,
        endpoint: &str,
    ) -> Result<bool> {
        let done = sqlx::query(
            "DELETE FROM project_sync WHERE user_id=$1 AND project=$2 AND endpoint_id=$3",
        )
        .bind(user.as_str())
        .bind(project)
        .bind(endpoint)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    pub(crate) async fn pinned_projects(&self, user: &UserKey) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT project FROM pinned_projects WHERE user_id=$1 ORDER BY project",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?)
    }

    /// Pins `project`, which stops it on every machine, or unpins it.
    pub(crate) async fn pin_project(
        &self,
        user: &UserKey,
        project: &str,
        pinned: bool,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        if pinned {
            sqlx::query(
                "INSERT INTO pinned_projects (user_id, project) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            )
            .bind(user.as_str())
            .bind(project)
            .execute(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM project_sync WHERE user_id=$1 AND project=$2")
                .bind(user.as_str())
                .bind(project)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("DELETE FROM pinned_projects WHERE user_id=$1 AND project=$2")
                .bind(user.as_str())
                .bind(project)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The projects whose sync is paused.
    pub(crate) async fn paused_syncs(&self, user: &UserKey) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar(
                "SELECT project FROM paused_syncs WHERE user_id=$1 ORDER BY project",
            )
            .bind(user.as_str())
            .fetch_all(&self.pool)
            .await?,
        )
    }

    /// Pauses or resumes `project`'s sync everywhere; false when unchanged.
    pub(crate) async fn pause_sync(
        &self,
        user: &UserKey,
        project: &str,
        paused: bool,
    ) -> Result<bool> {
        let query = if paused {
            "INSERT INTO paused_syncs (user_id, project) VALUES ($1, $2) ON CONFLICT DO NOTHING"
        } else {
            "DELETE FROM paused_syncs WHERE user_id=$1 AND project=$2"
        };
        let done = sqlx::query(query)
            .bind(user.as_str())
            .bind(project)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The projects pinned to the top of the sidebar, oldest pin first.
    pub(crate) async fn projects_on_top(&self, user: &UserKey) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT project FROM projects_on_top WHERE user_id=$1
             ORDER BY pinned_at, project",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?)
    }

    /// Pins or unpins `project` at the top of the sidebar; repinning keeps its place.
    pub(crate) async fn pin_project_to_top(
        &self,
        user: &UserKey,
        project: &str,
        pinned: bool,
    ) -> Result<()> {
        let sql = if pinned {
            "INSERT INTO projects_on_top (user_id, project) VALUES ($1, $2)
             ON CONFLICT DO NOTHING"
        } else {
            "DELETE FROM projects_on_top WHERE user_id=$1 AND project=$2"
        };
        sqlx::query(sql)
            .bind(user.as_str())
            .bind(project)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;

    fn row(project: &str, endpoint: &str, folder: &str) -> SyncRow {
        SyncRow {
            project: project.into(),
            endpoint: endpoint.into(),
            folder: folder.into(),
        }
    }

    #[tokio::test]
    async fn pinning_a_project_stops_it_everywhere() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let other = UserKey::for_test("bob");
        store
            .set_project_sync(&user, &row("game", "a", "/srv/one"))
            .await
            .unwrap();
        store
            .set_project_sync(&user, &row("game", "a", "/srv/game"))
            .await
            .unwrap();
        store
            .set_project_sync(&user, &row("game", "b", "/srv/game"))
            .await
            .unwrap();
        store
            .set_project_sync(&user, &row("site", "a", "/srv/site"))
            .await
            .unwrap();
        store
            .set_project_sync(&other, &row("game", "c", "/srv/game"))
            .await
            .unwrap();
        assert_eq!(
            store.project_syncs(&user).await.unwrap(),
            [
                row("game", "a", "/srv/game"),
                row("game", "b", "/srv/game"),
                row("site", "a", "/srv/site"),
            ]
        );
        assert!(store.remove_project_sync(&user, "site", "a").await.unwrap());
        assert!(!store.remove_project_sync(&user, "site", "a").await.unwrap());

        store.pin_project(&user, "game", true).await.unwrap();
        assert!(store.project_syncs(&user).await.unwrap().is_empty());
        assert_eq!(store.pinned_projects(&user).await.unwrap(), ["game"]);
        assert_eq!(store.project_syncs(&other).await.unwrap().len(), 1);
        store.pin_project(&user, "game", false).await.unwrap();
        assert!(store.pinned_projects(&user).await.unwrap().is_empty());
    }
}
