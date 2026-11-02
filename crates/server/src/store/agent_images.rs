//! Images users sent to coding agent sessions, kept so a session's log can
//! show them again. Each is a file without a chat; see `files`.

use super::Store;
use crate::{
    protocol::{AgentImage, image_key},
    user::UserKey,
};
use anyhow::Result;
use uuid::Uuid;

/// The session on a machine that images belong to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentSessionOf<'a> {
    pub(crate) machine_id: &'a str,
    /// The node's id for the session.
    pub(crate) session: &'a str,
}

/// An image to keep, under the name the session's log gives it.
pub(crate) struct NewAgentImage<'a> {
    pub(crate) name: &'a str,
    pub(crate) mime: &'a str,
    pub(crate) data: &'a [u8],
}

impl Store {
    /// Keeps `image` for the session `of`.
    pub(crate) async fn save_agent_image(
        &self,
        user: &UserKey,
        of: AgentSessionOf<'_>,
        image: NewAgentImage<'_>,
    ) -> Result<()> {
        let id = Uuid::new_v4().simple().to_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO files (id, user_id, name, mime, data, size) VALUES ($1, $2, $3, $4, '', $5)",
        )
        .bind(&id)
        .bind(user.as_str())
        .bind(image.name)
        .bind(image.mime)
        .bind(i64::try_from(image.data.len())?)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO agent_images (file_id, user_id, machine_id, session, name, key)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(user.as_str())
        .bind(of.machine_id)
        .bind(of.session)
        .bind(image.name)
        .bind(image_key(image.data))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.put_blob(&id, image.data).await
    }

    /// The images `user` sent to the session `of`, oldest first.
    pub(crate) async fn agent_images(
        &self,
        user: &UserKey,
        of: AgentSessionOf<'_>,
    ) -> Result<Vec<AgentImage>> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT a.name, a.file_id, f.mime, a.key FROM agent_images a JOIN files f ON f.id = a.file_id
             WHERE a.user_id = $1 AND a.machine_id = $2 AND a.session = $3 ORDER BY a.id",
        )
        .bind(user.as_str())
        .bind(of.machine_id)
        .bind(of.session)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(name, file_id, mime, key)| AgentImage {
                name,
                file_id,
                mime,
                key,
            })
            .collect())
    }

    /// Gives the session `to` the images of `from`, which it goes on from.
    pub(crate) async fn carry_agent_images(
        &self,
        user: &UserKey,
        from: AgentSessionOf<'_>,
        to: AgentSessionOf<'_>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_images (file_id, user_id, machine_id, session, name, key)
             SELECT file_id, user_id, $4, $5, name, key FROM agent_images
             WHERE user_id = $1 AND machine_id = $2 AND session = $3 ORDER BY id",
        )
        .bind(user.as_str())
        .bind(from.machine_id)
        .bind(from.session)
        .bind(to.machine_id)
        .bind(to.session)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;

    const ONE: AgentSessionOf<'static> = AgentSessionOf {
        machine_id: "build-box",
        session: "a1b2c3",
    };

    fn png(name: &str) -> NewAgentImage<'_> {
        NewAgentImage {
            name,
            mime: "image/png",
            data: b"\x89PNG",
        }
    }

    #[tokio::test]
    async fn a_sessions_images_read_back_for_their_owner_and_carry_over() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        store
            .save_agent_image(&alice, ONE, png("a.png"))
            .await
            .unwrap();
        store
            .save_agent_image(&alice, ONE, png("b.png"))
            .await
            .unwrap();
        let listed = store.agent_images(&alice, ONE).await.unwrap();
        let names: Vec<&str> = listed.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a.png", "b.png"]);
        assert_eq!(listed[0].mime, "image/png");
        assert_eq!(listed[0].key, image_key(b"\x89PNG"));
        let part = store
            .file_part(&alice, &listed[0].file_id, 0, 10)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(part.data, b"\x89PNG");
        assert!(store.agent_images(&bob, ONE).await.unwrap().is_empty());
        assert!(
            store
                .file_part(&bob, &listed[0].file_id, 0, 10)
                .await
                .unwrap()
                .is_none()
        );

        let next = AgentSessionOf {
            session: "d4e5f6",
            ..ONE
        };
        store.carry_agent_images(&alice, ONE, next).await.unwrap();
        assert_eq!(store.agent_images(&alice, next).await.unwrap(), listed);
    }
}
