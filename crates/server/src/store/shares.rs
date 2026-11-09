//! Shared sessions: frozen copies other users open by token.

use super::{
    Store,
    chats::SESSION_TITLE,
    messages::insert_steps,
    updates::{Readers, UpdateOp, log_chat, log_messages},
};
use crate::{account::user::UserKey, agent::steps};
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::core::Conversation;
use sqlx::Row;
use std::collections::HashMap;
use uuid::Uuid;

/// A shared copy of a session.
pub struct Share {
    pub token: String,
    pub title: String,
    pub conversation: Conversation,
    pub shared_at: DateTime<Utc>,
}

/// What `Store::copy_share` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopiedShare {
    Copied,
    /// No share has that token.
    NotFound,
    /// The user already has a session with the new id.
    Taken,
}

impl Store {
    /// Shares session `chat` as `conversation`, replacing an earlier copy and keeping its token; `None` when missing.
    pub(crate) async fn share_session(
        &self,
        user: &UserKey,
        chat: Uuid,
        conversation: &Conversation,
    ) -> Result<Option<Share>> {
        let row = sqlx::query(&format!(
            "INSERT INTO shares (token, user_id, chat_id, title, conversation)
             SELECT $1, m.user_id, c.id, {SESSION_TITLE}, $4
             FROM chats c JOIN chat_members m ON m.chat_id = c.id
             WHERE m.user_id = $2 AND c.id = $3 AND c.kind = 'session'
             ON CONFLICT (chat_id) DO UPDATE SET title=excluded.title,
                 conversation=excluded.conversation, shared_at=now()
             RETURNING token, title, shared_at"
        ))
        .bind(Uuid::new_v4().simple().to_string())
        .bind(user.as_ref())
        .bind(chat)
        .bind(serde_json::to_string(conversation)?)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(Share {
                token: row.try_get(0)?,
                title: row.try_get(1)?,
                conversation: conversation.clone(),
                shared_at: row.try_get(2)?,
            })
        })
        .transpose()
    }

    /// Stops sharing session `chat`; `false` when it was not shared.
    pub(crate) async fn unshare_session(&self, user: &UserKey, chat: Uuid) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM shares WHERE user_id=$1 AND chat_id=$2")
            .bind(user.as_ref())
            .bind(chat)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted > 0)
    }

    /// The share with `token`, whoever made it.
    pub async fn share(&self, token: &str) -> Result<Option<Share>> {
        let row =
            sqlx::query("SELECT token, title, conversation, shared_at FROM shares WHERE token=$1")
                .bind(token)
                .fetch_optional(&self.pool)
                .await?;
        row.map(|row| {
            Ok(Share {
                token: row.try_get(0)?,
                title: row.try_get(1)?,
                conversation: serde_json::from_str(row.try_get(2)?)?,
                shared_at: row.try_get(3)?,
            })
        })
        .transpose()
    }

    /// Copies share `token` into a new session `chat` of the user, under its title.
    pub(crate) async fn copy_share(
        &self,
        user: &UserKey,
        token: &str,
        chat: Uuid,
    ) -> Result<CopiedShare> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT title, conversation FROM shares WHERE token=$1")
            .bind(token)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(CopiedShare::NotFound);
        };
        let title: String = row.try_get(0)?;
        let raw: String = row.try_get(1)?;
        let conversation: Conversation = serde_json::from_str(&raw)?;
        let inserted = sqlx::query(
            "INSERT INTO chats (id, kind, title, titled_at) VALUES ($1, 'session', $2, now())
             ON CONFLICT DO NOTHING",
        )
        .bind(chat)
        .bind(title)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if inserted == 0 {
            tx.rollback().await?;
            return Ok(CopiedShare::Taken);
        }

        sqlx::query("INSERT INTO chat_members (chat_id, user_id) VALUES ($1, $2)")
            .bind(chat)
            .bind(user.as_ref())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO sessions (chat_id, context) VALUES ($1, $2)")
            .bind(chat)
            .bind(raw)
            .execute(&mut *tx)
            .await?;
        let copied = steps::steps(
            conversation.entries(),
            None,
            &steps::CallNotes::default(),
            &HashMap::new(),
        );
        let ids = insert_steps(&mut tx, chat, user.as_ref(), &copied.steps, None).await?;
        let readers = Readers::User(user.as_ref());
        let mut bumped = log_chat(&mut tx, chat, UpdateOp::Chat, readers).await?;
        bumped.extend(log_messages(&mut tx, chat, &ids, UpdateOp::Message, readers).await?);
        tx.commit().await?;
        self.announce(&bumped);
        Ok(CopiedShare::Copied)
    }
}

#[cfg(test)]
#[path = "../tests/store_shares.rs"]
mod tests;
