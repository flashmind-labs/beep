//! Project groups: folders a user lists projects under, matched by the apps.

use super::Store;
use crate::{account::user::UserKey, protocol::ProjectGroup};
use anyhow::Result;
use uuid::Uuid;

/// A group to save: a new one without `id`.
pub(crate) struct SaveGroup<'a> {
    pub(crate) id: Option<Uuid>,
    pub(crate) name: &'a str,
    pub(crate) paths: &'a [String],
}

impl Store {
    /// The user's groups, oldest first.
    pub(crate) async fn project_groups(&self, user: &UserKey) -> Result<Vec<ProjectGroup>> {
        let rows: Vec<(Uuid, String, Vec<String>)> = sqlx::query_as(
            "SELECT id, name, paths FROM project_groups WHERE user_id=$1
             ORDER BY created_at, id",
        )
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, paths)| ProjectGroup { id, name, paths })
            .collect())
    }

    /// Makes a group or changes group `id`; `None` when missing.
    pub(crate) async fn save_project_group(
        &self,
        user: &UserKey,
        group: SaveGroup<'_>,
    ) -> Result<Option<ProjectGroup>> {
        let saved: Option<(Uuid, String, Vec<String>)> = match group.id {
            None => Some(
                sqlx::query_as(
                    "INSERT INTO project_groups (id, user_id, name, paths) VALUES ($1, $2, $3, $4)
                     RETURNING id, name, paths",
                )
                .bind(Uuid::new_v4())
                .bind(user.as_ref())
                .bind(group.name)
                .bind(group.paths)
                .fetch_one(&self.pool)
                .await?,
            ),
            Some(id) => {
                sqlx::query_as(
                    "UPDATE project_groups SET name=$3, paths=$4 WHERE id=$1 AND user_id=$2
                     RETURNING id, name, paths",
                )
                .bind(id)
                .bind(user.as_ref())
                .bind(group.name)
                .bind(group.paths)
                .fetch_optional(&self.pool)
                .await?
            }
        };
        Ok(saved.map(|(id, name, paths)| ProjectGroup { id, name, paths }))
    }

    /// Forgets the user's group `id`; false when there is none.
    pub(crate) async fn delete_project_group(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM project_groups WHERE id=$1 AND user_id=$2")
            .bind(id)
            .bind(user.as_ref())
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }
}

#[cfg(test)]
#[path = "../tests/store_project_groups.rs"]
mod tests;
