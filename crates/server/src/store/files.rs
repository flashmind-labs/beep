//! Files kept for a chat: ones Beep sent (until deletion or `FILE_KEEP`) and ones users sent (as long as their message).

use super::{
    Store,
    updates::{Readers, UpdateOp, log_messages},
};
use crate::{
    account::user::UserKey,
    protocol::{FilePart, FileRef, ImageRef, SentFiles, SentImages},
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
    /// Keeps `file` and returns its id, pruning the user's sent files older than `FILE_KEEP` on the way.
    pub(crate) async fn save_file(&self, user: &UserKey, file: NewFile<'_>) -> Result<String> {
        self.drop_expired_files(user).await?;
        let id = Uuid::new_v4().simple().to_string();
        sqlx::query(
            "INSERT INTO files (id, user_id, chat_id, name, mime, data)
             VALUES ($1, $2, $3, $4, $5, '')",
        )
        .bind(&id)
        .bind(user.as_ref())
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
        .bind(user.as_ref())
        .bind(FILE_KEEP.as_secs_f64())
        .fetch_all(&self.pool)
        .await?;
        self.blobs.delete(&expired).await
    }

    /// Keeps the pictures `user` sent with a message, in order, all or none; returns what clients see.
    pub(crate) async fn save_pictures(
        &self,
        user: &UserKey,
        of: PicturesOf,
        pictures: &[NewPicture<'_>],
    ) -> Result<Vec<ImageRef>> {
        let mut staged = Vec::with_capacity(pictures.len());
        let kept = self.stage_pictures(user, of, pictures, &mut staged).await;
        self.attach_staged(of, staged, kept).await
    }

    async fn stage_pictures(
        &self,
        user: &UserKey,
        of: PicturesOf,
        pictures: &[NewPicture<'_>],
        staged: &mut Vec<String>,
    ) -> Result<Vec<ImageRef>> {
        let mut saved = Vec::with_capacity(pictures.len());
        for (position, picture) in pictures.iter().enumerate() {
            let id = Uuid::new_v4().simple().to_string();
            sqlx::query(
                "INSERT INTO files (id, user_id, chat_id, name, mime, data,
                                    position, width, height)
                 VALUES ($1, $2, $3, $4, $5, '', $6, $7, $8)",
            )
            .bind(&id)
            .bind(user.as_ref())
            .bind(of.chat)
            .bind(format!("picture-{}", position + 1))
            .bind(picture.mime)
            .bind(i32::try_from(position)?)
            .bind(i32::try_from(picture.width)?)
            .bind(i32::try_from(picture.height)?)
            .execute(&self.pool)
            .await?;
            staged.push(id.clone());
            self.blobs.put(&id, picture.data).await?;
            saved.push(ImageRef {
                id,
                mime: picture.mime.to_owned(),
                width: picture.width,
                height: picture.height,
            });
        }
        Ok(saved)
    }

    /// Keeps the files `user` sent with a message, in order, all or none.
    pub(crate) async fn save_attachments(
        &self,
        user: &UserKey,
        of: PicturesOf,
        files: &[NewAttachment<'_>],
    ) -> Result<()> {
        let mut staged = Vec::with_capacity(files.len());
        let kept = self.stage_attachments(user, of, files, &mut staged).await;
        self.attach_staged(of, staged, kept).await
    }

    async fn stage_attachments(
        &self,
        user: &UserKey,
        of: PicturesOf,
        files: &[NewAttachment<'_>],
        staged: &mut Vec<String>,
    ) -> Result<()> {
        for (position, file) in files.iter().enumerate() {
            let id = Uuid::new_v4().simple().to_string();
            sqlx::query(
                "INSERT INTO files (id, user_id, chat_id, name, mime, data,
                                    position, attached, size)
                 VALUES ($1, $2, $3, $4, $5, '', $6, true, $7)",
            )
            .bind(&id)
            .bind(user.as_ref())
            .bind(of.chat)
            .bind(file.name)
            .bind(file.mime)
            .bind(i32::try_from(position)?)
            .bind(i64::try_from(file.data.len())?)
            .execute(&self.pool)
            .await?;
            staged.push(id.clone());
            self.blobs.put(&id, file.data).await?;
        }
        Ok(())
    }

    /// Attaches `staged` rows to message `of` once `kept` succeeds, else drops them; crash leftovers expire like sent files.
    async fn attach_staged<T>(
        &self,
        of: PicturesOf,
        staged: Vec<String>,
        kept: Result<T>,
    ) -> Result<T> {
        let attached = match kept {
            Ok(kept) => self.attach(of, &staged).await.map(|()| kept),
            Err(error) => Err(error),
        };
        if attached.is_err()
            && !staged.is_empty()
            && let Err(error) = self.drop_staged(&staged).await
        {
            log::warn!("files: unable to drop files of a failed save: {error:#}");
        }
        attached
    }

    async fn attach(&self, of: PicturesOf, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE files SET message_id = $1 WHERE id = ANY($2)")
            .bind(of.message)
            .bind(ids)
            .execute(&mut *tx)
            .await?;
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

    async fn drop_staged(&self, ids: &[String]) -> Result<()> {
        sqlx::query("DELETE FROM files WHERE id = ANY($1) AND message_id IS NULL")
            .bind(ids)
            .execute(&self.pool)
            .await?;
        self.blobs.delete(ids).await
    }

    /// Marks the prompt `of.message` of `user`'s chat as said aloud.
    pub(crate) async fn mark_spoken(&self, user: &UserKey, of: PicturesOf) -> Result<()> {
        sqlx::query(
            "UPDATE messages SET spoken = true
             WHERE id = $1 AND chat_id = $2 AND author_id = $3",
        )
        .bind(of.message)
        .bind(of.chat)
        .bind(user.as_ref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Send times in ms of each spoken prompt in `user`'s session `chat`, oldest first.
    pub(crate) async fn spoken_turns(&self, user: &UserKey, chat: Uuid) -> Result<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT floor(extract(epoch FROM sent_at) * 1000)::bigint
             FROM messages
             WHERE chat_id = $1 AND author_id = $2 AND spoken
             ORDER BY id",
        )
        .bind(chat)
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?)
    }

    /// Non-picture files `user` sent in session `chat`, by message time in ms.
    pub(crate) async fn session_files(&self, user: &UserKey, chat: Uuid) -> Result<Vec<SentFiles>> {
        let rows = sqlx::query(
            "SELECT floor(extract(epoch FROM m.sent_at) * 1000)::bigint AS message,
                 f.id, f.name, f.mime, f.size
             FROM files f JOIN messages m ON m.id = f.message_id
             WHERE f.user_id = $1 AND f.chat_id = $2 AND f.attached
             ORDER BY m.id, f.position",
        )
        .bind(user.as_ref())
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

    /// Stores file `id`'s bytes, dropping its row on failure so no file is listed without them.
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

    /// Pictures `user` sent in session `chat`, by message time in ms.
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
        .bind(user.as_ref())
        .bind(chat)
        .fetch_all(&self.pool)
        .await?;
        by_message(&rows)
    }

    /// Pictures sent in group `chat`, by message id; the caller checks membership.
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

    /// Up to `len` bytes of file `id` from `offset`; `None` if missing, expired, or not readable by `user` (owner, or group members for group pictures).
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
        .bind(user.as_ref())
        .bind(id)
        .bind(FILE_KEEP.as_secs_f64())
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
#[path = "../tests/store_files.rs"]
mod tests;
