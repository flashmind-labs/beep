//! Token usage and cost per user, day, model and kind of turn.

use super::Store;
use crate::metering::CallUsage;
use anyhow::Result;

impl Store {
    /// Adds one model call to the user's usage for today.
    pub(crate) async fn add_token_usage(&self, user: &str, call: &CallUsage) -> Result<()> {
        let usage = &call.tokens;
        sqlx::query(
            "INSERT INTO token_usage (user_id, day, model, agent, calls, prompt_tokens,
                 cached_tokens, cache_write_tokens, completion_tokens, cost)
             VALUES ($1, (now() AT TIME ZONE 'UTC')::date, $2, $3, 1, $4, $5, $6, $7, $8)
             ON CONFLICT (user_id, day, model, agent) DO UPDATE SET
                 calls = token_usage.calls + 1,
                 prompt_tokens = token_usage.prompt_tokens + excluded.prompt_tokens,
                 cached_tokens = token_usage.cached_tokens + excluded.cached_tokens,
                 cache_write_tokens = token_usage.cache_write_tokens + excluded.cache_write_tokens,
                 completion_tokens = token_usage.completion_tokens + excluded.completion_tokens,
                 cost = token_usage.cost + excluded.cost",
        )
        .bind(user)
        .bind(&call.model)
        // The `agent` column keeps its name; only research writes a value.
        .bind(if call.research { "research" } else { "" })
        .bind(i64::from(usage.prompt_tokens))
        .bind(i64::from(usage.cache_read_tokens))
        .bind(i64::from(usage.cache_creation_tokens))
        .bind(i64::from(usage.completion_tokens))
        .bind(call.cost)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
