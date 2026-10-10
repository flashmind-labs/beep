//! Files kept for a chat: ones Beep sent the user, kept until the session is
//! deleted or `FILE_KEEP` passes, and pictures users sent, kept as long as
//! their session turn or group. The rows hold what each file is; `Blobs`
//! holds the bytes.

use super::{Store, seconds};
use crate::{
    protocol::{FilePart, ImageRef, SentImages},
    user::UserKey,
};
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

/// A picture a user sent with a message.
pub struct NewPicture<'a> {
    pub mime: &'a str,
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// The message pictures came with.
#[derive(Debug, Clone, Copy)]
pub enum PicturesOf<'a> {
    /// A session turn, by its user message's time in milliseconds.
    Turn { session_id: &'a str, at: i64 },
    /// A group message, by its `seq`.
    Group { group_id: &'a str, seq: i64 },
}

impl Store {
    /// Keeps `file` and returns its id. Drops the user's files older than
    /// `FILE_KEEP` on the way, so nothing else has to sweep them; pictures
    /// stay with their chat.
    pub(crate) async fn save_file(&self, user: &UserKey, file: NewFile<'_>) -> Result<String> {
        let expired: Vec<String> = sqlx::query_scalar(
            "DELETE FROM files WHERE user_id=$1 AND turn_at IS NULL AND group_seq IS NULL
             AND created_at < now() - make_interval(secs => $2) RETURNING id",
        )
        .bind(user.as_str())
        .bind(seconds(FILE_KEEP))
        .fetch_all(&self.pool)
        .await?;
        self.blobs.delete(&expired).await?;
        let id = Uuid::new_v4().simple().to_string();
        sqlx::query(
            "INSERT INTO files (id, user_id, session_id, name, mime, data)
             VALUES ($1, $2, $3, $4, $5, '')",
        )
        .bind(&id)
        .bind(user.as_str())
        .bind(file.session_id)
        .bind(file.name)
        .bind(file.mime)
        .execute(&self.pool)
        .await?;
        self.put_blob(&id, file.data).await?;
        Ok(id)
    }

    /// Keeps the pictures `user` sent with a message, in order, and returns
    /// what clients see of them.
    pub(crate) async fn save_pictures(
        &self,
        user: &UserKey,
        of: PicturesOf<'_>,
        pictures: &[NewPicture<'_>],
    ) -> Result<Vec<ImageRef>> {
        let (chat, turn_at, group_seq) = match of {
            PicturesOf::Turn { session_id, at } => (session_id, Some(at), None),
            PicturesOf::Group { group_id, seq } => (group_id, None, Some(seq)),
        };
        let mut saved = Vec::with_capacity(pictures.len());
        for (position, picture) in pictures.iter().enumerate() {
            let id = Uuid::new_v4().simple().to_string();
            sqlx::query(
                "INSERT INTO files (id, user_id, session_id, name, mime, data,
                                    turn_at, group_seq, position, width, height)
                 VALUES ($1, $2, $3, $4, $5, '', $6, $7, $8, $9, $10)",
            )
            .bind(&id)
            .bind(user.as_str())
            .bind(chat)
            .bind(format!("picture-{}", position + 1))
            .bind(picture.mime)
            .bind(turn_at)
            .bind(group_seq)
            .bind(i32::try_from(position)?)
            .bind(i32::try_from(picture.width)?)
            .bind(i32::try_from(picture.height)?)
            .execute(&self.pool)
            .await?;
            self.put_blob(&id, picture.data).await?;
            saved.push(ImageRef {
                id,
                mime: picture.mime.to_owned(),
                width: picture.width,
                height: picture.height,
            });
        }
        Ok(saved)
    }

    /// Stores the bytes of file `id`, dropping its row when that fails so
    /// no file is listed without them.
    async fn put_blob(&self, id: &str, data: &[u8]) -> Result<()> {
        if let Err(error) = self.blobs.put(id, data).await {
            sqlx::query("DELETE FROM files WHERE id=$1")
                .bind(id)
                .execute(&self.pool)
                .await?;
            return Err(error);
        }
        Ok(())
    }

    /// The pictures `user` sent in session `session_id`, by turn.
    pub(crate) async fn session_pictures(
        &self,
        user: &UserKey,
        session_id: &str,
    ) -> Result<Vec<SentImages>> {
        let rows = sqlx::query(
            "SELECT turn_at AS message, id, mime, width, height FROM files
             WHERE user_id=$1 AND session_id=$2 AND turn_at IS NOT NULL
             ORDER BY turn_at, position",
        )
        .bind(user.as_str())
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        by_message(&rows)
    }

    /// The pictures sent in group `group_id`, by message. The caller checks
    /// membership.
    pub(crate) async fn group_pictures(&self, group_id: &str) -> Result<Vec<SentImages>> {
        let rows = sqlx::query(
            "SELECT group_seq AS message, id, mime, width, height FROM files
             WHERE session_id=$1 AND group_seq IS NOT NULL
             ORDER BY group_seq, position",
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;
        by_message(&rows)
    }

    /// Up to `len` bytes of file `id` from `offset` on. `None` when there is
    /// no such file, it has expired, or `user` may not read it: a file is
    /// its owner's, and a group's pictures are its members'.
    pub(crate) async fn file_part(
        &self,
        user: &UserKey,
        id: &str,
        offset: u64,
        len: u64,
    ) -> Result<Option<FilePart>> {
        let readable: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM files f WHERE f.id=$2 AND (
                (f.group_seq IS NULL AND f.user_id=$1
                    AND (f.turn_at IS NOT NULL OR f.created_at >= now() - make_interval(secs => $3)))
                OR (f.group_seq IS NOT NULL AND EXISTS(
                    SELECT 1 FROM group_members m WHERE m.group_id=f.session_id AND m.user_id=$1))))",
        )
        .bind(user.as_str())
        .bind(id)
        .bind(seconds(FILE_KEEP))
        .fetch_one(&self.pool)
        .await?;
        if !readable {
            return Ok(None);
        }
        self.blobs.part(id, offset, len).await
    }
}

/// Rows of `message, id, mime, width, height`, sorted by message, grouped.
fn by_message(rows: &[sqlx::postgres::PgRow]) -> Result<Vec<SentImages>> {
    let mut sent: Vec<SentImages> = Vec::new();
    for row in rows {
        let message: i64 = row.try_get("message")?;
        let width: i32 = row.try_get("width")?;
        let height: i32 = row.try_get("height")?;
        let image = ImageRef {
            id: row.try_get("id")?,
            mime: row.try_get("mime")?,
            width: u32::try_from(width)?,
            height: u32::try_from(height)?,
        };
        match sent.last_mut() {
            Some(last) if last.message == message => last.images.push(image),
            _ => sent.push(SentImages {
                message,
                images: vec![image],
            }),
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{NewMessage, TestDb, TurnsToDelete},
        user::User,
    };
    use chrono::DateTime;
    use flashmind::core::{Conversation, ConversationEntry};

    fn picture(data: &[u8]) -> NewPicture<'_> {
        NewPicture {
            mime: "image/jpeg",
            data,
            width: 640,
            height: 480,
        }
    }

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

    #[tokio::test]
    async fn session_pictures_load_by_turn_and_go_with_the_turn_or_chat() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let mut chat = Conversation::new();
        for (at, prompt) in [(1000, "one"), (2000, "two")] {
            let mut entries = [
                ConversationEntry::user(prompt),
                ConversationEntry::assistant(format!("re: {prompt}")),
            ];
            for (i, entry) in entries.iter_mut().enumerate() {
                entry.timestamp = DateTime::from_timestamp_millis(at + i as i64).unwrap();
            }
            chat.entries_mut().extend(entries);
        }
        sqlx::query("INSERT INTO sessions (user_id, id, conversation, updated_at) VALUES ($1, 's1', $2, now())")
            .bind(alice.as_str())
            .bind(serde_json::to_string(&chat).unwrap())
            .execute(store.pool())
            .await
            .unwrap();
        let turn = |at| PicturesOf::Turn {
            session_id: "s1",
            at,
        };
        let first = store
            .save_pictures(&alice, turn(1000), &[picture(b"one"), picture(b"two")])
            .await
            .unwrap();
        let second = store
            .save_pictures(&alice, turn(2000), &[picture(b"three")])
            .await
            .unwrap();
        assert_eq!(
            (first[0].width, first[0].height, first[0].mime.as_str()),
            (640, 480, "image/jpeg")
        );

        let listed = store.session_pictures(&alice, "s1").await.unwrap();
        assert_eq!(
            listed,
            [
                SentImages {
                    message: 1000,
                    images: first.clone()
                },
                SentImages {
                    message: 2000,
                    images: second.clone()
                },
            ]
        );
        let read = store.file_part(&alice, &first[1].id, 0, 100).await.unwrap();
        assert_eq!(read.unwrap().data, b"two");
        assert!(store.session_pictures(&bob, "s1").await.unwrap().is_empty());
        assert!(
            store
                .file_part(&bob, &first[0].id, 0, 100)
                .await
                .unwrap()
                .is_none()
        );

        // Pictures outlive the 30 days sent files get.
        sqlx::query("UPDATE files SET created_at = now() - interval '90 days'")
            .execute(store.pool())
            .await
            .unwrap();
        store
            .save_file(
                &alice,
                NewFile {
                    session_id: "s2",
                    name: "a.pdf",
                    mime: "application/pdf",
                    data: b"pdf",
                },
            )
            .await
            .unwrap();
        assert_eq!(store.session_pictures(&alice, "s1").await.unwrap().len(), 2);

        // Deleting a turn takes its pictures; deleting the chat takes the rest.
        store
            .delete_turn(&alice, "s1", 1000, TurnsToDelete::One)
            .await
            .unwrap();
        assert_eq!(
            store.session_pictures(&alice, "s1").await.unwrap(),
            [SentImages {
                message: 2000,
                images: second.clone()
            }]
        );
        assert!(
            store
                .file_part(&alice, &first[0].id, 0, 100)
                .await
                .unwrap()
                .is_none()
        );
        store
            .delete_sessions(&alice, vec!["s1".into()])
            .await
            .unwrap();
        assert!(
            store
                .session_pictures(&alice, "s1")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .file_part(&alice, &second[0].id, 0, 100)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn group_pictures_are_every_members_until_the_group_goes() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let user = UserKey::for_test("user");
        store
            .create_group(&alice, "g1", "Trip", "Alice")
            .await
            .unwrap();
        let token = store.group_invite(&alice, "g1", false).await.unwrap();
        store
            .join_group(&bob, &token.unwrap(), "Bob")
            .await
            .unwrap();
        let posted = store
            .post_to_group(
                &alice,
                NewMessage {
                    group_id: "g1",
                    request_id: "r1",
                    content: "look",
                },
            )
            .await
            .unwrap()
            .unwrap();
        let of = PicturesOf::Group {
            group_id: "g1",
            seq: posted.message.seq,
        };
        let saved = store
            .save_pictures(&alice, of, &[picture(b"view")])
            .await
            .unwrap();

        assert_eq!(
            store.group_pictures("g1").await.unwrap(),
            [SentImages {
                message: posted.message.seq,
                images: saved.clone()
            }]
        );
        let read = store.file_part(&bob, &saved[0].id, 0, 100).await.unwrap();
        assert_eq!(read.unwrap().data, b"view");
        assert!(
            store
                .file_part(&user, &saved[0].id, 0, 100)
                .await
                .unwrap()
                .is_none()
        );

        // The sender leaving keeps them for the group; the last one out deletes them.
        store.leave_group(&alice, "g1").await.unwrap();
        assert!(
            store
                .file_part(&bob, &saved[0].id, 0, 100)
                .await
                .unwrap()
                .is_some()
        );
        store.leave_group(&bob, "g1").await.unwrap();
        assert!(store.group_pictures("g1").await.unwrap().is_empty());
    }
}
