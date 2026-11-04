//! What Beep did in the user's apps.

use super::Store;
use crate::account::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;

/// Actions older than this are dropped when new ones are saved.
const KEEP_DAYS: i32 = 30;

/// One thing Beep did, such as "Gmail: send email (To: a@b.c)".
#[derive(Debug, Clone)]
pub struct ActionRow {
    pub done_at: DateTime<Utc>,
    pub description: String,
}

impl Store {
    pub(crate) async fn record_actions(
        &self,
        user: &UserKey,
        descriptions: &[String],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for description in descriptions {
            sqlx::query("INSERT INTO actions (user_id, description) VALUES ($1, $2)")
                .bind(user.as_str())
                .bind(description)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "DELETE FROM actions WHERE user_id=$1 AND done_at < now() - make_interval(days => $2)",
        )
        .bind(user.as_str())
        .bind(KEEP_DAYS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The latest `limit` actions, newest first.
    pub(crate) async fn recent_actions(
        &self,
        user: &UserKey,
        limit: i64,
    ) -> Result<Vec<ActionRow>> {
        sqlx::query(
            "SELECT done_at, description FROM actions WHERE user_id=$1
             ORDER BY done_at DESC LIMIT $2",
        )
        .bind(user.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| {
            Ok(ActionRow {
                done_at: row.try_get(0)?,
                description: row.try_get(1)?,
            })
        })
        .collect()
    }
}
