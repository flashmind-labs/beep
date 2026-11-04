//! A merged account's usage counts: same keys add up.

use anyhow::Result;
use sqlx::PgConnection;

/// A table of counts, by key.
pub(super) struct Summed {
    pub(super) table: &'static str,
    /// Beside `user_id`, what a row counts for.
    key: &'static [&'static str],
    /// Columns the target's row keeps as they are.
    kept: &'static [&'static str],
    /// Columns that add up. A missing count (NULL) on one side is the other.
    added: &'static [&'static str],
}

impl Summed {
    /// Every column but `user_id`.
    #[cfg(test)]
    pub(super) fn columns(&self) -> impl Iterator<Item = &'static str> {
        self.key.iter().chain(self.kept).chain(self.added).copied()
    }
}

pub(super) const SUMMED: &[Summed] = &[
    Summed {
        table: "token_usage",
        key: &["day", "model", "agent"],
        kept: &[],
        added: &[
            "calls",
            "prompt_tokens",
            "cached_tokens",
            "cache_write_tokens",
            "completion_tokens",
            "cost",
        ],
    },
    Summed {
        table: "agent_usage",
        key: &["endpoint_id", "session", "day"],
        kept: &["agent", "cwd", "agent_session"],
        added: &["cost_usd", "tokens", "unpriced_tokens", "turns"],
    },
    Summed {
        table: "agent_model_usage",
        key: &["day", "agent", "model"],
        kept: &[],
        added: &["tokens"],
    },
    Summed {
        table: "agent_session_usage",
        key: &[
            "endpoint_id",
            "agent",
            "session",
            "hour",
            "model",
            "billing",
        ],
        kept: &["cwd"],
        added: &[
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "cost_usd",
            "unpriced_tokens",
        ],
    },
];

pub(super) async fn move_usage(tx: &mut PgConnection, source: &str, target: &str) -> Result<()> {
    for summed in SUMMED {
        let table = summed.table;
        let columns = [summed.key, summed.kept, summed.added].concat().join(", ");
        let key = summed.key.join(", ");
        let added = summed
            .added
            .iter()
            .map(|c| {
                format!("{c} = COALESCE({table}.{c} + excluded.{c}, {table}.{c}, excluded.{c})")
            })
            .collect::<Vec<_>>()
            .join(", ");
        sqlx::query(&format!(
            "INSERT INTO {table} (user_id, {columns})
             SELECT $2, {columns} FROM {table} WHERE user_id=$1
             ON CONFLICT (user_id, {key}) DO UPDATE SET {added}"
        ))
        .bind(source)
        .bind(target)
        .execute(&mut *tx)
        .await?;
        sqlx::query(&format!("DELETE FROM {table} WHERE user_id=$1"))
            .bind(source)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}
