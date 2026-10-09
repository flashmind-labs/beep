//! Files Beep sent the user in a chat, kept until the session is deleted or
//! `FILE_KEEP` passes.

use super::{Store, seconds};
use crate::{protocol::FilePart, user::UserKey};
use anyhow::Result;
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

/// How long a sent file stays downloadable.
const FILE_KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// A file to keep for a chat.
pub struct NewFile<'a> {
    pub session_id: &'a str,
    pub name: &'a str,
    pub mime: &'a str,
    pub data: &'a [u8],
}

impl Store {
    /// Keeps `file` and returns its id. Drops the user's files older than
    /// `FILE_KEEP` on the way, so nothing else has to sweep them.
    pub(crate) async fn save_file(&self, user: &UserKey, file: NewFile<'_>) -> Result<String> {
        sqlx::query(
            "DELETE FROM files WHERE user_id=$1 AND created_at < now() - make_interval(secs => $2)",
        )
        .bind(user.as_str())
        .bind(seconds(FILE_KEEP))
        .execute(&self.pool)
        .await?;
        let id = Uuid::new_v4().simple().to_string();
        sqlx::query(
            "INSERT INTO files (id, user_id, session_id, name, mime, data) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(user.as_str())
        .bind(file.session_id)
        .bind(file.name)
        .bind(file.mime)
        .bind(file.data)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Up to `len` bytes of the user's file `id` from `offset` on. `None`
    /// when there is no such file or it has expired.
    pub(crate) async fn file_part(
        &self,
        user: &UserKey,
        id: &str,
        offset: u64,
        len: u64,
    ) -> Result<Option<FilePart>> {
        let row = sqlx::query(
            "SELECT substring(data FROM $3::int FOR $4::int), length(data) FROM files
             WHERE user_id=$1 AND id=$2 AND created_at >= now() - make_interval(secs => $5)",
        )
        .bind(user.as_str())
        .bind(id)
        // SQL counts from 1.
        .bind(i64::try_from(offset)?.saturating_add(1))
        .bind(i64::try_from(len)?)
        .bind(seconds(FILE_KEEP))
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let size: i32 = row.try_get(1)?;
            Ok(FilePart {
                data: row.try_get(0)?,
                size: u64::try_from(size)?,
            })
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::{store::TestDb, user::User};

    #[tokio::test]
    async fn a_file_reads_back_in_parts_for_its_owner_only() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = User::for_test(store.clone(), "alice");
        let bob = User::for_test(store, "bob");
        let data: Vec<u8> = (0..=255).collect();
        let id = alice
            .save_file("s1", "pass.pdf", "application/pdf", &data)
            .await
            .unwrap();
        let first = alice.file_part(&id, 0, 100).await.unwrap().unwrap();
        assert_eq!(first.size, 256);
        assert_eq!(first.data, data[..100]);
        let last = alice.file_part(&id, 200, 100).await.unwrap().unwrap();
        assert_eq!(last.data, data[200..]);
        let past = alice.file_part(&id, 300, 100).await.unwrap().unwrap();
        assert!(past.data.is_empty());
        assert!(bob.file_part(&id, 0, 100).await.unwrap().is_none());

        alice.delete_session("s1").await.unwrap();
        assert!(alice.file_part(&id, 0, 100).await.unwrap().is_none());
    }
}
