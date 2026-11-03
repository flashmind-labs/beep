//! Copies of the pictures coding agent sessions show, by the `image_key` of
//! their bytes, so `GetAgentImage` answers while the machine is offline.
//! Each is a file without a chat (see `files`): it expires after
//! `FILE_KEEP`, and a user's copies stay under `MAX_USER_BYTES`, the oldest
//! going first.

use super::{Store, files::FILE_KEEP, seconds};
use crate::{protocol::image_key, user::UserKey};
use anyhow::{Result, bail};
use sqlx::Row;
use uuid::Uuid;

/// The largest picture kept: what a node sends in one frame.
const MAX_AGENT_IMAGE: usize = 7 * 1024 * 1024;
/// The most bytes of pictures kept for one user.
const MAX_USER_BYTES: i64 = 256 * 1024 * 1024;

/// The session a picture was seen in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentImageOf<'a> {
    pub(crate) machine_id: &'a str,
    /// The node's id for the session.
    pub(crate) session: &'a str,
}

/// A picture's type and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentImageBytes {
    pub(crate) mime: String,
    pub(crate) data: Vec<u8>,
}

impl Store {
    /// Keeps a copy of `image` for `user`, unless one with the same bytes
    /// is kept already.
    pub(crate) async fn save_agent_image(
        &self,
        user: &UserKey,
        of: AgentImageOf<'_>,
        image: &AgentImageBytes,
    ) -> Result<()> {
        if image.data.len() > MAX_AGENT_IMAGE {
            bail!("the picture is over {} MiB", MAX_AGENT_IMAGE >> 20);
        }
        if !image.mime.starts_with("image/") {
            bail!("{} is not a picture type", image.mime);
        }
        // Expired copies go first, so their keys are free again.
        self.drop_expired_files(user).await?;
        let key = image_key(&image.data);
        let id = Uuid::new_v4().simple().to_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO files (id, user_id, name, mime, data, size) VALUES ($1, $2, $3, $4, '', $5)",
        )
        .bind(&id)
        .bind(user.as_str())
        .bind(format!("image-{key}"))
        .bind(&image.mime)
        .bind(i64::try_from(image.data.len())?)
        .execute(&mut *tx)
        .await?;
        let added = sqlx::query(
            "INSERT INTO agent_images (user_id, key, file_id, machine_id, session)
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (user_id, key) DO NOTHING",
        )
        .bind(user.as_str())
        .bind(&key)
        .bind(&id)
        .bind(of.machine_id)
        .bind(of.session)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if added == 0 {
            tx.rollback().await?;
            return Ok(());
        }
        tx.commit().await?;
        self.put_blob(&id, &image.data).await?;
        self.trim_agent_images(user, MAX_USER_BYTES).await
    }

    /// The kept copy of `user`'s picture `key`, while it has not expired.
    pub(crate) async fn agent_image(
        &self,
        user: &UserKey,
        key: &str,
    ) -> Result<Option<AgentImageBytes>> {
        let row = sqlx::query(
            "SELECT f.id, f.mime, f.size FROM agent_images a JOIN files f ON f.id = a.file_id
             WHERE a.user_id = $1 AND a.key = $2
               AND f.created_at >= now() - make_interval(secs => $3)",
        )
        .bind(user.as_str())
        .bind(key)
        .bind(seconds(FILE_KEEP))
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let id: String = row.try_get("id")?;
        let size: i64 = row.try_get("size")?;
        let Some(part) = self.blobs.part(&id, 0, u64::try_from(size)?).await? else {
            return Ok(None);
        };
        // A copy whose bytes are not all there yet is not one.
        if part.size != u64::try_from(size)? || part.data.len() != usize::try_from(size)? {
            return Ok(None);
        }
        Ok(Some(AgentImageBytes {
            mime: row.try_get("mime")?,
            data: part.data,
        }))
    }

    /// Drops `user`'s oldest copies past `max` bytes.
    async fn trim_agent_images(&self, user: &UserKey, max: i64) -> Result<()> {
        let dropped: Vec<String> = sqlx::query_scalar(
            "DELETE FROM files WHERE id IN (
                SELECT file_id FROM (
                    SELECT a.file_id,
                        sum(f.size) OVER (ORDER BY f.created_at DESC, f.id DESC) AS total
                    FROM agent_images a JOIN files f ON f.id = a.file_id
                    WHERE a.user_id = $1) newest
                WHERE total > $2)
             RETURNING id",
        )
        .bind(user.as_str())
        .bind(max)
        .fetch_all(&self.pool)
        .await?;
        self.blobs.delete(&dropped).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;

    const ONE: AgentImageOf<'static> = AgentImageOf {
        machine_id: "build-box",
        session: "a1b2c3",
    };

    fn png(fill: u8, size: usize) -> AgentImageBytes {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.resize(size, fill);
        AgentImageBytes {
            mime: "image/png".into(),
            data,
        }
    }

    #[tokio::test]
    async fn a_picture_reads_back_by_its_key_for_its_owner_only() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let shot = png(1, 100);
        let key = image_key(&shot.data);
        assert_eq!(store.agent_image(&alice, &key).await.unwrap(), None);
        store.save_agent_image(&alice, ONE, &shot).await.unwrap();
        // The same bytes again keep the one copy.
        store.save_agent_image(&alice, ONE, &shot).await.unwrap();
        assert_eq!(
            store.agent_image(&alice, &key).await.unwrap(),
            Some(shot.clone())
        );
        assert_eq!(store.agent_image(&bob, &key).await.unwrap(), None);
        let copies: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_images")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(copies, 1);
        // Expired with the user's other files.
        sqlx::query("UPDATE files SET created_at = now() - interval '31 days'")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(store.agent_image(&alice, &key).await.unwrap(), None);
        store.save_agent_image(&alice, ONE, &shot).await.unwrap();
        assert_eq!(store.agent_image(&alice, &key).await.unwrap(), Some(shot));
    }

    #[tokio::test]
    async fn too_large_or_not_a_picture_is_refused() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let big = png(2, MAX_AGENT_IMAGE + 1);
        assert!(store.save_agent_image(&alice, ONE, &big).await.is_err());
        let page = AgentImageBytes {
            mime: "text/html".into(),
            data: b"<p>hi</p>".to_vec(),
        };
        assert!(store.save_agent_image(&alice, ONE, &page).await.is_err());
    }

    #[tokio::test]
    async fn a_users_oldest_pictures_go_past_the_limit() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let shots: Vec<AgentImageBytes> = (0..4).map(|n| png(n, 1000)).collect();
        for shot in &shots {
            store.save_agent_image(&alice, ONE, shot).await.unwrap();
            // Apart in time, so the order is plain.
            sqlx::query("UPDATE files SET created_at = created_at - interval '1 second'")
                .execute(&store.pool)
                .await
                .unwrap();
        }
        store.save_agent_image(&bob, ONE, &shots[0]).await.unwrap();
        store.trim_agent_images(&alice, 2500).await.unwrap();
        let kept = |user: &UserKey, shot: &AgentImageBytes| {
            let (store, user, key) = (store.clone(), user.clone(), image_key(&shot.data));
            async move { store.agent_image(&user, &key).await.unwrap().is_some() }
        };
        assert!(!kept(&alice, &shots[0]).await);
        assert!(!kept(&alice, &shots[1]).await);
        assert!(kept(&alice, &shots[2]).await);
        assert!(kept(&alice, &shots[3]).await);
        assert!(kept(&bob, &shots[0]).await, "another user's stay");
    }
}
