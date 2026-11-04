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
        .bind(user.as_str())
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
                .bind(user.as_str())
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
                .bind(user.as_str())
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
            .bind(user.as_str())
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|p| (*p).to_owned()).collect()
    }

    #[tokio::test]
    async fn groups_are_saved_renamed_and_forgotten_per_user() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let other = UserKey::for_test("bob");
        let first = paths(&["work"]);
        let field = store
            .save_project_group(
                &user,
                SaveGroup {
                    id: None,
                    name: "work",
                    paths: &first,
                },
            )
            .await
            .unwrap()
            .expect("a new group is made");
        let second = paths(&["/srv/project", "work/tools"]);
        let tools = store
            .save_project_group(
                &user,
                SaveGroup {
                    id: None,
                    name: "tools",
                    paths: &second,
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store.project_groups(&user).await.unwrap(),
            [field.clone(), tools.clone()]
        );
        assert!(store.project_groups(&other).await.unwrap().is_empty());

        let renamed = paths(&["work", "games"]);
        let changed = store
            .save_project_group(
                &user,
                SaveGroup {
                    id: Some(field.id),
                    name: "Field",
                    paths: &renamed,
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(changed.id, field.id);
        assert_eq!(changed.name, "Field");
        assert_eq!(changed.paths, renamed);
        // Another user's group is not theirs to change or forget.
        let taken = store
            .save_project_group(
                &other,
                SaveGroup {
                    id: Some(field.id),
                    name: "mine",
                    paths: &first,
                },
            )
            .await
            .unwrap();
        assert!(taken.is_none());
        assert!(!store.delete_project_group(&other, field.id).await.unwrap());

        assert!(store.delete_project_group(&user, field.id).await.unwrap());
        assert!(!store.delete_project_group(&user, field.id).await.unwrap());
        assert_eq!(store.project_groups(&user).await.unwrap(), [tools]);
    }
}
