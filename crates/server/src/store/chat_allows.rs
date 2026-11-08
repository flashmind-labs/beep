//! Actions allowed for the rest of one chat via "Allow for this session".

use super::Store;
use crate::account::user::UserKey;
use anyhow::Result;
use sqlx::Row;
use uuid::Uuid;

/// One action allowed in a chat: its app slug and tool slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatAllow {
    pub(crate) app: String,
    pub(crate) tool: String,
}

impl Store {
    /// Lets `allow` run without asking in `chat` from now on.
    pub(crate) async fn allow_in_chat(
        &self,
        user: &UserKey,
        chat: Uuid,
        allow: &ChatAllow,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO chat_allows (chat_id, user_id, app, tool) VALUES ($1, $2, $3, $4)
             ON CONFLICT (chat_id, app, tool) DO NOTHING",
        )
        .bind(chat)
        .bind(user.as_ref())
        .bind(&allow.app)
        .bind(&allow.tool)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// What the user allowed for the rest of `chat`.
    pub(crate) async fn chat_allows(&self, user: &UserKey, chat: Uuid) -> Result<Vec<ChatAllow>> {
        let rows = sqlx::query(
            "SELECT app, tool FROM chat_allows WHERE user_id=$1 AND chat_id=$2
             ORDER BY allowed_at, app, tool",
        )
        .bind(user.as_ref())
        .bind(chat)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ChatAllow {
                    app: row.try_get(0)?,
                    tool: row.try_get(1)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/store_chat_allows.rs"]
mod tests;
