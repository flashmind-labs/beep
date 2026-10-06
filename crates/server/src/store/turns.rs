//! What a session keeps about each turn besides the conversation: failed
//! tool calls, memory scores and the model that answered.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use flashmind::types::ReasoningLevel;
use sqlx::Row;
use std::collections::{HashMap, HashSet};

/// The model that answered a turn, and the thinking Jev picked for it when
/// Auto picked the model.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnModel {
    pub model: String,
    pub auto_thinking: Option<ReasoningLevel>,
}

impl Store {
    /// Records tool calls that failed, since the conversation keeps only their output.
    pub(crate) async fn record_tool_failures(
        &self,
        user: &UserKey,
        session: &str,
        call_ids: Vec<String>,
    ) -> Result<()> {
        if call_ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO tool_failures (user_id, session_id, call_id)
             SELECT $1, $2, unnest($3::text[]) ON CONFLICT DO NOTHING",
        )
        .bind(user.as_str())
        .bind(session)
        .bind(&call_ids)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Ids of the session's tool calls that failed.
    pub(crate) async fn tool_failures(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<HashSet<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT call_id FROM tool_failures WHERE user_id=$1 AND session_id=$2",
        )
        .bind(user.as_str())
        .bind(session)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids.into_iter().collect())
    }

    /// Records Jev's memory score for the turn whose user message was sent at
    /// `turn_at`, in milliseconds since the epoch.
    pub(crate) async fn record_memory_score(
        &self,
        user: &UserKey,
        session: &str,
        turn_at: i64,
        probability: f64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO memory_scores (user_id, session_id, turn_at, probability)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, session_id, turn_at) DO UPDATE SET probability=excluded.probability",
        )
        .bind(user.as_str())
        .bind(session)
        .bind(turn_at)
        .bind(probability)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records the model that answered the turn whose user message was sent
    /// at `turn_at`, in milliseconds since the epoch.
    pub(crate) async fn record_turn_model(
        &self,
        user: &UserKey,
        session: &str,
        turn_at: i64,
        model: &TurnModel,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO turn_models (user_id, session_id, turn_at, model, thinking)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (user_id, session_id, turn_at)
             DO UPDATE SET model=excluded.model, thinking=excluded.thinking",
        )
        .bind(user.as_str())
        .bind(session)
        .bind(turn_at)
        .bind(&model.model)
        .bind(model.auto_thinking.map(|level| level.to_string()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The session's models by the time of each turn's user message.
    pub(crate) async fn turn_models(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<HashMap<i64, TurnModel>> {
        let rows = sqlx::query(
            "SELECT turn_at, model, thinking FROM turn_models WHERE user_id=$1 AND session_id=$2",
        )
        .bind(user.as_str())
        .bind(session)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                let thinking: Option<String> = r.try_get(2)?;
                let model = TurnModel {
                    model: r.try_get(1)?,
                    auto_thinking: thinking.and_then(|level| level.parse().ok()),
                };
                Ok((r.try_get(0)?, model))
            })
            .collect()
    }

    /// The session's memory scores by the time of each turn's user message.
    pub(crate) async fn memory_scores(
        &self,
        user: &UserKey,
        session: &str,
    ) -> Result<HashMap<i64, f64>> {
        let rows = sqlx::query(
            "SELECT turn_at, probability FROM memory_scores WHERE user_id=$1 AND session_id=$2",
        )
        .bind(user.as_str())
        .bind(session)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| Ok((r.try_get(0)?, r.try_get(1)?)))
            .collect()
    }
}
