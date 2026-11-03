//! The chat about one kept triage update, made on first open.

use super::{
    Store,
    chats::{SessionChat, ensure_session},
    events::{event_row, triage_row},
    messages::{Step, StepBody, insert_steps},
    updates::{Readers, UpdateOp, log_chat, log_messages},
};
use crate::{
    events::Verdict,
    store::{EventRow, TriageRow},
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::Utc;
use flashmind::core::Conversation;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

/// An update with the events it covers, to open a chat about.
#[derive(Debug, Clone)]
pub struct UpdateDetail {
    pub update: TriageRow,
    /// Those still stored, oldest first.
    pub events: Vec<UpdateEvent>,
}

/// An event an update covers, with what its app sent.
#[derive(Debug, Clone)]
pub struct UpdateEvent {
    pub row: EventRow,
    /// The record as the app or Composio sent it.
    pub raw: Value,
}

/// What a new update chat starts with.
pub(crate) struct UpdateChatSeed<'a> {
    pub(crate) title: &'a str,
    /// What the agent reads, ending in Beep's message.
    pub(crate) conversation: &'a Conversation,
    /// Beep's message, the one message the apps show.
    pub(crate) message: &'a str,
}

/// The chat an update has, and whether this call made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UpdateChat {
    pub(crate) chat: Uuid,
    pub(crate) made: bool,
}

impl Store {
    /// Update `id` of `user` with its events; `None` when not theirs or not passed on.
    pub(crate) async fn update_detail(
        &self,
        user: &UserKey,
        id: i64,
    ) -> Result<Option<UpdateDetail>> {
        let Some(row) = sqlx::query(
            "SELECT id, decided_at, decision, urgency, summary, reason, refs, delivered_at
             FROM event_triage WHERE id=$1 AND user_id=$2 AND decision=$3",
        )
        .bind(id)
        .bind(user.as_str())
        .bind(Verdict::Inform.as_str())
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let update = triage_row(&row)?;
        let rows = sqlx::query(
            "SELECT id, app, kind, summary, detail, ids, occurred_at, state, raw FROM app_events
             WHERE user_id=$1 AND id = ANY($2) ORDER BY id",
        )
        .bind(user.as_str())
        .bind(&update.refs)
        .fetch_all(&self.pool)
        .await?;
        let events = rows
            .iter()
            .map(|row| {
                Ok(UpdateEvent {
                    row: event_row(row)?,
                    raw: row.try_get("raw")?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Some(UpdateDetail { update, events }))
    }

    /// The chat about update `id`, made from `seed` if needed, marking it seen; `None` when not the user's.
    pub(crate) async fn open_update_chat(
        &self,
        user: &UserKey,
        id: i64,
        seed: &UpdateChatSeed<'_>,
    ) -> Result<Option<UpdateChat>> {
        let mut tx = self.pool.begin().await?;
        // Locking the update makes a second caller wait and find the chat.
        let found: Option<i64> = sqlx::query_scalar(
            "UPDATE event_triage SET delivered_at = COALESCE(delivered_at, now())
             WHERE id=$1 AND user_id=$2 AND decision=$3 RETURNING id",
        )
        .bind(id)
        .bind(user.as_str())
        .bind(Verdict::Inform.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        if found.is_none() {
            tx.rollback().await?;
            return Ok(None);
        }
        let existing: Option<Uuid> =
            sqlx::query_scalar("SELECT chat_id FROM update_chats WHERE triage_id=$1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(chat) = existing {
            tx.commit().await?;
            return Ok(Some(UpdateChat { chat, made: false }));
        }
        let chat = Uuid::new_v4();
        if ensure_session(&mut tx, user.as_str(), chat).await? != SessionChat::New {
            bail!("a fresh chat id was already taken");
        }
        sqlx::query("UPDATE chats SET title=$2, titled_at=now() WHERE id=$1")
            .bind(chat)
            .bind(seed.title)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO sessions (chat_id, context) VALUES ($1, $2)")
            .bind(chat)
            .bind(serde_json::to_string(seed.conversation)?)
            .execute(&mut *tx)
            .await?;
        // Beep's message stands alone, before any turn, as a routine's
        // notice does.
        let step = Step {
            sent_at: Utc::now(),
            body: StepBody::Assistant {
                content: seed.message.to_owned(),
            },
        };
        let ids = insert_steps(&mut tx, chat, user.as_str(), &[step], None).await?;
        sqlx::query("INSERT INTO update_chats (triage_id, chat_id) VALUES ($1, $2)")
            .bind(id)
            .bind(chat)
            .execute(&mut *tx)
            .await?;
        let readers = Readers::User(user.as_str());
        let mut bumped = log_chat(&mut tx, chat, UpdateOp::Chat, readers).await?;
        bumped.extend(log_messages(&mut tx, chat, &ids, UpdateOp::Message, readers).await?);
        tx.commit().await?;
        self.announce(&bumped);
        Ok(Some(UpdateChat { chat, made: true }))
    }
}
