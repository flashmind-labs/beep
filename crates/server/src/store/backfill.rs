//! One-time backfill of message rows from stored transcripts, between migrations 0029 and 0030 (needs Rust to parse the JSON).

use super::{
    messages::{Step, StepBody, TurnRecord, insert_steps},
    updates::{Readers, UpdateOp, log_messages},
};
use crate::{agent::steps, apps::permissions, protocol::HistoryEntry};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use flashmind::core::Conversation;
use sqlx::{PgConnection, PgPool, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Sessions read per transaction.
const BATCH: i64 = 50;

/// Fills in the messages of every session not yet done; no-op past 0030.
pub(super) async fn run(pool: &PgPool) -> Result<()> {
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.columns
         WHERE table_schema = current_schema() AND table_name = 'sessions'
           AND column_name = 'messages_backfilled')",
    )
    .fetch_one(pool)
    .await?;
    if !pending {
        return Ok(());
    }
    let mut done = 0;
    loop {
        let mut tx = pool.begin().await?;
        let chats: Vec<Uuid> = sqlx::query_scalar(
            "SELECT chat_id FROM sessions WHERE NOT messages_backfilled
             ORDER BY chat_id LIMIT $1 FOR UPDATE",
        )
        .bind(BATCH)
        .fetch_all(&mut *tx)
        .await?;
        if chats.is_empty() {
            break;
        }
        for chat in &chats {
            backfill(&mut tx, *chat)
                .await
                .with_context(|| format!("backfilling session {chat}"))?;
        }
        tx.commit().await?;
        done += chats.len();
    }
    if done > 0 {
        log::info!("backfilled the messages of {done} session(s)");
    }
    Ok(())
}

async fn backfill(tx: &mut PgConnection, chat: Uuid) -> Result<()> {
    let row = sqlx::query(
        "SELECT s.conversation, m.user_id FROM sessions s
         JOIN chat_members m ON m.chat_id = s.chat_id WHERE s.chat_id = $1",
    )
    .bind(chat)
    .fetch_one(&mut *tx)
    .await?;
    let raw: String = row.try_get(0)?;
    let owner: String = row.try_get(1)?;
    let transcript: Conversation = serde_json::from_str(&raw)?;
    let session = chat.to_string();

    let failures: HashSet<String> = sqlx::query_scalar(
        "SELECT call_id FROM tool_failures WHERE user_id = $1 AND session_id = $2",
    )
    .bind(&owner)
    .bind(&session)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let mut turns: steps::TurnsByTime = HashMap::new();
    let models = sqlx::query(
        "SELECT turn_at, model, thinking FROM turn_models WHERE user_id = $1 AND session_id = $2",
    )
    .bind(&owner)
    .bind(&session)
    .fetch_all(&mut *tx)
    .await?;
    for row in models {
        let thinking: Option<String> = row.try_get(2)?;
        turns.insert(
            row.try_get(0)?,
            TurnRecord {
                model: row.try_get(1)?,
                auto_thinking: thinking.and_then(|level| level.parse().ok()),
                ..TurnRecord::default()
            },
        );
    }
    let scores = sqlx::query(
        "SELECT turn_at, probability FROM memory_scores WHERE user_id = $1 AND session_id = $2",
    )
    .bind(&owner)
    .bind(&session)
    .fetch_all(&mut *tx)
    .await?;
    for row in scores {
        turns.entry(row.try_get(0)?).or_default().memory_score = Some(row.try_get(1)?);
    }

    let calls = steps::CallNotes {
        failed: failures,
        ..steps::CallNotes::default()
    };
    let mut saved = steps::steps(transcript.entries(), None, &calls, &turns).steps;
    // The backfill runs before the projects tables exist, so no chat it
    // reads is a project chat.
    if let Some(HistoryEntry::Approval { query_id, approval }) =
        permissions::waiting_approval(&transcript, false)
    {
        let sent_at = transcript
            .entries()
            .last()
            .map_or_else(Utc::now, |entry| entry.timestamp);
        saved.push(Step {
            sent_at,
            body: StepBody::Approval { query_id, approval },
        });
    }
    let ids = insert_steps(tx, chat, &owner, &saved, None).await?;

    // Pictures named their turn by the time of its opening message.
    let openers: Vec<(DateTime<Utc>, i64)> = saved
        .iter()
        .zip(&ids)
        .filter(|(step, _)| step.body.opens_turn())
        .map(|(step, id)| (step.sent_at, *id))
        .collect();
    for (sent_at, id) in openers {
        sqlx::query(
            "UPDATE files SET message_id = $3
             WHERE chat_id = $1 AND message_id IS NULL AND turn_at = $2",
        )
        .bind(chat)
        .bind(sent_at.timestamp_millis())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    log_messages(tx, chat, &ids, UpdateOp::Message, Readers::Members).await?;
    sqlx::query("UPDATE sessions SET messages_backfilled = TRUE WHERE chat_id = $1")
        .bind(chat)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/store_backfill.rs"]
mod tests;
