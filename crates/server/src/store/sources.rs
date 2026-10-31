//! The web pages each turn's reply used, kept with the message that opened
//! the turn so the apps show them again when a chat is loaded.

use super::{
    Store,
    chats::touch,
    updates::{Readers, UpdateOp, log_messages},
};
use crate::protocol::{MAX_SOURCES, TurnSources, WebSource};
use anyhow::Result;
use sqlx::Row;
use uuid::Uuid;

impl Store {
    /// Adds `sources` to the turn opened by message `turn` of `chat`, after
    /// the ones it has, as a resumed turn does. Pages it lists already are
    /// skipped, and a turn keeps at most `MAX_SOURCES`. The chat counts as
    /// changed when any were added.
    pub(crate) async fn save_sources(
        &self,
        chat: Uuid,
        turn: i64,
        sources: &[WebSource],
    ) -> Result<()> {
        if sources.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        // Locks the turn's opening message, so two saves cannot take the
        // same positions.
        let opens: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM messages WHERE id = $1 AND chat_id = $2 AND turn_id = id FOR UPDATE",
        )
        .bind(turn)
        .bind(chat)
        .fetch_optional(&mut *tx)
        .await?;
        if opens.is_none() {
            return Ok(());
        }
        let kept: Vec<String> = sqlx::query_scalar(
            "SELECT url FROM turn_sources WHERE message_id = $1 ORDER BY position",
        )
        .bind(turn)
        .fetch_all(&mut *tx)
        .await?;
        let first = i32::try_from(kept.len())?;
        let room = MAX_SOURCES.saturating_sub(kept.len());
        let new = sources
            .iter()
            .filter(|source| !kept.contains(&source.url))
            .take(room);
        let mut added = false;
        for (position, source) in (first..).zip(new) {
            sqlx::query(
                "INSERT INTO turn_sources (message_id, position, url, title)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(turn)
            .bind(position)
            .bind(&source.url)
            .bind(&source.title)
            .execute(&mut *tx)
            .await?;
            added = true;
        }
        if !added {
            return Ok(());
        }
        // The chat changed, so a device holding the turn without them
        // fetches it again.
        touch(&mut tx, chat).await?;
        let bumped =
            log_messages(&mut tx, chat, &[turn], UpdateOp::Message, Readers::Members).await?;
        tx.commit().await?;
        self.announce(&bumped);
        Ok(())
    }

    /// The sources of each turn of `chat` that has some, oldest first, by
    /// when its opening message was sent in milliseconds. The caller checks
    /// the user may read it.
    pub(crate) async fn session_sources(&self, chat: Uuid) -> Result<Vec<TurnSources>> {
        let rows = sqlx::query(
            "SELECT floor(extract(epoch FROM m.sent_at) * 1000)::bigint AS message,
                 s.url, s.title
             FROM turn_sources s JOIN messages m ON m.id = s.message_id
             WHERE m.chat_id = $1
             ORDER BY m.id, s.position",
        )
        .bind(chat)
        .fetch_all(&self.pool)
        .await?;
        let mut turns: Vec<TurnSources> = Vec::new();
        for row in &rows {
            let message: i64 = row.try_get("message")?;
            let source = WebSource {
                url: row.try_get("url")?,
                title: row.try_get("title")?,
            };
            match turns.last_mut() {
                Some(last) if last.message == message => last.sources.push(source),
                _ => turns.push(TurnSources {
                    message,
                    sources: vec![source],
                }),
            }
        }
        Ok(turns)
    }
}
