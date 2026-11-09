//! Copies of agent session pictures by `image_key`, so `GetAgentImage` works while the machine is offline.

use super::{Store, files::FILE_KEEP};
use crate::account::user::UserKey;
use anyhow::Result;
use sqlx::Row;

/// A picture's type and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentImageBytes {
    pub(crate) mime: String,
    pub(crate) data: Vec<u8>,
}

impl Store {
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
        .bind(user.as_ref())
        .bind(key)
        .bind(FILE_KEEP.as_secs_f64())
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
}
