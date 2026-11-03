//! Storage for kept file bytes under the `files` row id; `PgBlobs` keeps them in the row, an object store could replace it.

use crate::protocol::FilePart;
use anyhow::Result;
use async_trait::async_trait;
use sqlx::{PgPool, Row};

#[async_trait]
pub trait Blobs: Send + Sync {
    /// Stores `data` as the bytes of file `id`, whose `files` row exists.
    async fn put(&self, id: &str, data: &[u8]) -> Result<()>;

    /// Up to `len` bytes of blob `id` from `offset`, with its total size.
    async fn part(&self, id: &str, offset: u64, len: u64) -> Result<Option<FilePart>>;

    /// Drops the bytes of files whose rows were deleted.
    async fn delete(&self, ids: &[String]) -> Result<()>;
}

/// Keeps the bytes in `files.data`.
pub struct PgBlobs {
    pool: PgPool,
}

impl PgBlobs {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Blobs for PgBlobs {
    async fn put(&self, id: &str, data: &[u8]) -> Result<()> {
        sqlx::query("UPDATE files SET data=$2 WHERE id=$1")
            .bind(id)
            .bind(data)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn part(&self, id: &str, offset: u64, len: u64) -> Result<Option<FilePart>> {
        // A bytea holds at most 1 GB, so a clamped offset is still past the
        // end and reads nothing.
        let offset = i32::try_from(offset).unwrap_or(i32::MAX);
        let len = i32::try_from(len).unwrap_or(i32::MAX);
        let row = sqlx::query(
            "SELECT substring(data FROM $2 FOR $3), length(data) FROM files WHERE id=$1",
        )
        .bind(id)
        // SQL counts from 1.
        .bind(offset.saturating_add(1))
        .bind(len)
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

    async fn delete(&self, _ids: &[String]) -> Result<()> {
        // The bytes went with their rows.
        Ok(())
    }
}
