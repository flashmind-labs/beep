//! Files kept for a chat: ones Beep sent the user, kept until the session is
//! deleted or `FILE_KEEP` passes, and pictures and other files users sent,
//! kept as long as their message. The rows hold what each file is; `Blobs`
//! holds the bytes.

use super::{
    Store, seconds,
    updates::{Readers, UpdateOp, log_messages},
};
use crate::{
    protocol::{FilePart, FileRef, ImageRef, SentFiles, SentImages},
    user::UserKey,
};
use anyhow::Result;
use sqlx::{Row, postgres::PgRow};
use std::time::Duration;
use uuid::Uuid;

/// How long a sent file stays downloadable.
pub(super) const FILE_KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// A file to keep for a chat.
pub struct NewFile<'a> {
    pub chat: Uuid,
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

/// A file other than a picture that a user sent with a message.
pub struct NewAttachment<'a> {
    pub name: &'a str,
    pub mime: &'a str,
    pub data: &'a [u8],
}

/// The message pictures came with.
#[derive(Debug, Clone, Copy)]
pub struct PicturesOf {
    pub chat: Uuid,
    pub message: i64,
}

impl Store {
    /// Keeps `file` and returns its id. Drops the user's files older than
    /// `FILE_KEEP` on the way, so nothing else has to sweep them; pictures
    /// stay with their chat.
    pub(crate) async fn save_file(&self, user: &UserKey, file: NewFile<'_>) -> Result<String> {
        self.drop_expired_files(user).await?;
        let id = Uuid::new_v4().simple().to_string();
        sqlx::query(
            "INSERT INTO files (id, user_id, chat_id, name, mime, data)
             VALUES ($1, $2, $3, $4, $5, '')",
        )
        .bind(&id)
        .bind(user.as_str())
        .bind(file.chat)
        .bind(file.name)
        .bind(file.mime)
        .execute(&self.pool)
        .await?;
        self.put_blob(&id, file.data).await?;
        Ok(id)
    }

    /// Drops `user`'s files without a message older than `FILE_KEEP`.
    pub(super) async fn drop_expired_files(&self, user: &UserKey) -> Result<()> {
        let expired: Vec<String> = sqlx::query_scalar(
            "DELETE FROM files WHERE user_id=$1 AND message_id IS NULL
             AND created_at < now() - make_interval(secs => $2) RETURNING id",
        )
        .bind(user.as_str())
        .bind(seconds(FILE_KEEP))
        .fetch_all(&self.pool)
        .await?;
        self.blobs.delete(&expired).await
    }

    /// Keeps the pictures `user` sent with a message, in order, and returns
    /// what clients see of them. The message goes out again with them.
    pub(crate) async fn save_pictures(
        &self,
        user: &UserKey,
        of: PicturesOf,
        pictures: &[NewPicture<'_>],
    ) -> Result<Vec<ImageRef>> {
        let mut saved = Vec::with_capacity(pictures.len());
        for (position, picture) in pictures.iter().enumerate() {
            let id = Uuid::new_v4().simple().to_string();
            sqlx::query(
                "INSERT INTO files (id, user_id, chat_id, message_id, name, mime, data,
                                    position, width, height)
                 VALUES ($1, $2, $3, $4, $5, $6, '', $7, $8, $9)",
            )
            .bind(&id)
            .bind(user.as_str())
            .bind(of.chat)
            .bind(of.message)
            .bind(format!("picture-{}", position + 1))
            .bind(picture.mime)
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
        if !saved.is_empty() {
            let mut tx = self.pool.begin().await?;
            let bumped = log_messages(
                &mut tx,
                of.chat,
                &[of.message],
                UpdateOp::Message,
                Readers::Members,
            )
            .await?;
            tx.commit().await?;
            self.announce(&bumped);
        }
        Ok(saved)
    }

    /// Keeps the files `user` sent with a message, in order. The message goes
    /// out again, so other devices fetch them.
    pub(crate) async fn save_attachments(
        &self,
        user: &UserKey,
        of: PicturesOf,
        files: &[NewAttachment<'_>],
    ) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        for (position, file) in files.iter().enumerate() {
            let id = Uuid::new_v4().simple().to_string();
            sqlx::query(
                "INSERT INTO files (id, user_id, chat_id, message_id, name, mime, data,
                                    position, attached, size)
                 VALUES ($1, $2, $3, $4, $5, $6, '', $7, true, $8)",
            )
            .bind(&id)
            .bind(user.as_str())
            .bind(of.chat)
            .bind(of.message)
            .bind(file.name)
            .bind(file.mime)
            .bind(i32::try_from(position)?)
            .bind(i64::try_from(file.data.len())?)
            .execute(&self.pool)
            .await?;
            self.put_blob(&id, file.data).await?;
        }
        let mut tx = self.pool.begin().await?;
        let bumped = log_messages(
            &mut tx,
            of.chat,
            &[of.message],
            UpdateOp::Message,
            Readers::Members,
        )
        .await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(())
    }

    /// Marks the prompt `of.message` of `user`'s chat as said aloud.
    pub(crate) async fn mark_spoken(&self, user: &UserKey, of: PicturesOf) -> Result<()> {
        sqlx::query(
            "UPDATE messages SET spoken = true
             WHERE id = $1 AND chat_id = $2 AND author_id = $3",
        )
        .bind(of.message)
        .bind(of.chat)
        .bind(user.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// When each spoken prompt of `user`'s session `chat` was sent, in
    /// milliseconds, oldest first.
    pub(crate) async fn spoken_turns(&self, user: &UserKey, chat: Uuid) -> Result<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT floor(extract(epoch FROM sent_at) * 1000)::bigint
             FROM messages
             WHERE chat_id = $1 AND author_id = $2 AND spoken
             ORDER BY id",
        )
        .bind(chat)
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?)
    }

    /// The files other than pictures `user` sent in session `chat`, by the
    /// time of the message they came with in milliseconds.
    pub(crate) async fn session_files(&self, user: &UserKey, chat: Uuid) -> Result<Vec<SentFiles>> {
        let rows = sqlx::query(
            "SELECT floor(extract(epoch FROM m.sent_at) * 1000)::bigint AS message,
                 f.id, f.name, f.mime, f.size
             FROM files f JOIN messages m ON m.id = f.message_id
             WHERE f.user_id = $1 AND f.chat_id = $2 AND f.attached
             ORDER BY m.id, f.position",
        )
        .bind(user.as_str())
        .bind(chat)
        .fetch_all(&self.pool)
        .await?;
        let mut sent: Vec<SentFiles> = Vec::new();
        for row in &rows {
            let message: i64 = row.try_get("message")?;
            let size: i64 = row.try_get("size")?;
            let file = FileRef {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                mime: row.try_get("mime")?,
                size: u64::try_from(size)?,
            };
            match sent.last_mut() {
                Some(last) if last.message == message => last.files.push(file),
                _ => sent.push(SentFiles {
                    message,
                    files: vec![file],
                }),
            }
        }
        Ok(sent)
    }

    /// Stores the bytes of file `id`, dropping its row when that fails so
    /// no file is listed without them.
    pub(super) async fn put_blob(&self, id: &str, data: &[u8]) -> Result<()> {
        if let Err(error) = self.blobs.put(id, data).await {
            sqlx::query("DELETE FROM files WHERE id=$1")
                .bind(id)
                .execute(&self.pool)
                .await?;
            return Err(error);
        }
        Ok(())
    }

    /// The pictures `user` sent in session `chat`, by the time of the
    /// message they came with in milliseconds.
    pub(crate) async fn session_pictures(
        &self,
        user: &UserKey,
        chat: Uuid,
    ) -> Result<Vec<SentImages>> {
        let rows = sqlx::query(
            "SELECT floor(extract(epoch FROM m.sent_at) * 1000)::bigint AS message,
                 f.id, f.mime, f.width, f.height
             FROM files f JOIN messages m ON m.id = f.message_id
             WHERE f.user_id = $1 AND f.chat_id = $2 AND NOT f.attached
             ORDER BY m.id, f.position",
        )
        .bind(user.as_str())
        .bind(chat)
        .fetch_all(&self.pool)
        .await?;
        by_message(&rows)
    }

    /// The pictures sent in group `chat`, by message id. The caller checks
    /// membership.
    pub(crate) async fn group_pictures(&self, chat: Uuid) -> Result<Vec<SentImages>> {
        let rows = sqlx::query(
            "SELECT message_id AS message, id, mime, width, height FROM files
             WHERE chat_id = $1 AND message_id IS NOT NULL AND NOT attached
             ORDER BY message_id, position",
        )
        .bind(chat)
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
                (f.user_id=$1
                    AND (f.message_id IS NOT NULL OR f.created_at >= now() - make_interval(secs => $3)))
                OR (f.message_id IS NOT NULL AND EXISTS(
                    SELECT 1 FROM chat_members m JOIN chats c ON c.id = m.chat_id
                    WHERE m.chat_id = f.chat_id AND m.user_id = $1 AND c.kind = 'group'))))",
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
fn by_message(rows: &[PgRow]) -> Result<Vec<SentImages>> {
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
        store::{NewMessage, TestDb, TurnsToDelete, test_chat, test_turn},
        user::User,
    };

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
        let bob = User::for_test(store.clone(), "bob");
        let data: Vec<u8> = (0..=255).collect();
        let s1 = test_chat("s1");
        test_turn(&store, &UserKey::for_test("alice"), s1, "hi", 1000)
            .await
            .unwrap();
        let id = alice
            .save_file(s1, "pass.pdf", "application/pdf", &data)
            .await
            .unwrap();
        let first = alice.file_part(&id, 0, 100).await.unwrap().unwrap();
        assert_eq!(first.size, 256);
        assert_eq!(first.data, data[..100]);
        let last = alice.file_part(&id, 200, 100).await.unwrap().unwrap();
        assert_eq!(last.data, data[200..]);
        let past = alice.file_part(&id, 300, 100).await.unwrap().unwrap();
        assert!(past.data.is_empty());
        for offset in [u64::from(u32::MAX), u64::MAX] {
            let far = alice.file_part(&id, offset, 100).await.unwrap().unwrap();
            assert!(far.data.is_empty());
            assert_eq!(far.size, 256);
        }
        let rest = alice.file_part(&id, 200, u64::MAX).await.unwrap().unwrap();
        assert_eq!(rest.data, data[200..]);
        assert!(bob.file_part(&id, 0, 100).await.unwrap().is_none());

        alice.delete_session(s1).await.unwrap();
        assert!(alice.file_part(&id, 0, 100).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn session_pictures_load_by_turn_and_go_with_the_turn_or_chat() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let s1 = test_chat("s1");
        let one = test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
        let two = test_turn(&store, &alice, s1, "two", 2000).await.unwrap();
        let of = |message| PicturesOf { chat: s1, message };
        let first = store
            .save_pictures(&alice, of(one), &[picture(b"one"), picture(b"two")])
            .await
            .unwrap();
        let second = store
            .save_pictures(&alice, of(two), &[picture(b"three")])
            .await
            .unwrap();
        assert_eq!(
            (first[0].width, first[0].height, first[0].mime.as_str()),
            (640, 480, "image/jpeg")
        );

        let listed = store.session_pictures(&alice, s1).await.unwrap();
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
        assert!(store.session_pictures(&bob, s1).await.unwrap().is_empty());
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
                    chat: test_chat("s2"),
                    name: "a.pdf",
                    mime: "application/pdf",
                    data: b"pdf",
                },
            )
            .await
            .unwrap();
        assert_eq!(store.session_pictures(&alice, s1).await.unwrap().len(), 2);

        // Deleting a turn takes its pictures; deleting the chat takes the rest.
        store
            .delete_turn(&alice, s1, 1000, TurnsToDelete::One)
            .await
            .unwrap();
        assert_eq!(
            store.session_pictures(&alice, s1).await.unwrap(),
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
        store.delete_sessions(&alice, &[s1]).await.unwrap();
        assert!(store.session_pictures(&alice, s1).await.unwrap().is_empty());
        assert!(
            store
                .file_part(&alice, &second[0].id, 0, 100)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn sent_files_list_apart_from_pictures() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let s1 = test_chat("s1");
        let one = test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
        let of = PicturesOf {
            chat: s1,
            message: one,
        };
        store
            .save_pictures(&alice, of, &[picture(b"pic")])
            .await
            .unwrap();
        let lease = NewAttachment {
            name: "Lease 2026.pdf",
            mime: "application/pdf",
            data: b"%PDF-lease",
        };
        let rent = NewAttachment {
            name: "rent.csv",
            mime: "text/csv",
            data: b"may,1450",
        };
        store
            .save_attachments(&alice, of, &[lease, rent])
            .await
            .unwrap();

        let files = store.session_files(&alice, s1).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].message, 1000);
        let named: Vec<(&str, &str, u64)> = files[0]
            .files
            .iter()
            .map(|f| (f.name.as_str(), f.mime.as_str(), f.size))
            .collect();
        assert_eq!(
            named,
            [
                ("Lease 2026.pdf", "application/pdf", 10),
                ("rent.csv", "text/csv", 8)
            ]
        );
        let read = store
            .file_part(&alice, &files[0].files[1].id, 0, 100)
            .await
            .unwrap();
        assert_eq!(read.unwrap().data, b"may,1450");
        assert!(store.session_files(&bob, s1).await.unwrap().is_empty());

        // Pictures stay pictures: the files are not among them.
        let pictures = store.session_pictures(&alice, s1).await.unwrap();
        assert_eq!(pictures[0].images.len(), 1);
        let messages = store.chat_messages(s1).await.unwrap();
        let opened = messages.iter().find(|m| m.id == one).unwrap();
        assert_eq!(opened.images.len(), 1);

        store
            .delete_turn(&alice, s1, 1000, TurnsToDelete::One)
            .await
            .unwrap();
        assert!(store.session_files(&alice, s1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn group_pictures_are_every_members_until_the_group_goes() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let user = UserKey::for_test("user");
        let g1 = test_chat("g1");
        store
            .create_group(&alice, g1, "Trip", "Alice")
            .await
            .unwrap();
        let token = store.group_invite(&alice, g1, false).await.unwrap();
        store
            .join_group(&bob, &token.unwrap(), "Bob")
            .await
            .unwrap();
        let posted = store
            .post_to_group(
                &alice,
                NewMessage {
                    group_id: g1,
                    request_id: test_chat("r1"),
                    content: "look",
                },
            )
            .await
            .unwrap()
            .unwrap();
        let of = PicturesOf {
            chat: g1,
            message: posted.message.id,
        };
        let saved = store
            .save_pictures(&alice, of, &[picture(b"view")])
            .await
            .unwrap();

        assert_eq!(
            store.group_pictures(g1).await.unwrap(),
            [SentImages {
                message: posted.message.id,
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
        store.leave_group(&alice, g1).await.unwrap();
        assert!(
            store
                .file_part(&bob, &saved[0].id, 0, 100)
                .await
                .unwrap()
                .is_some()
        );
        store.leave_group(&bob, g1).await.unwrap();
        assert!(store.group_pictures(g1).await.unwrap().is_empty());
    }
}
