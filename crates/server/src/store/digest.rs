//! Emails flagged for the evening digest, and the digest routine itself.

use super::{Feed, Store, cron_jobs::encode_schedule};
use crate::{account::user::UserKey, apps::feeds::mail::Mailbox, routines::digest::DIGEST_KIND};
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::cron::CronJob;
use sqlx::Row;
use std::time::Duration;

/// An email waiting for the digest.
#[derive(Debug, Clone)]
pub struct AttentionItem {
    pub mailbox: Mailbox,
    pub message_id: String,
    pub received: DateTime<Utc>,
    /// The email as the digest shows it.
    pub text: String,
}

impl Store {
    /// Makes the digest routine `job` unless one exists; returns whether it did.
    pub(crate) async fn offer_digest(&self, user: &UserKey, job: &CronJob) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let offered =
            sqlx::query("UPDATE users SET digest_offered=TRUE WHERE id=$1 AND NOT digest_offered")
                .bind(user.as_ref())
                .execute(&mut *tx)
                .await?
                .rows_affected();
        if offered == 0 {
            return Ok(false);
        }

        let label = job.metadata["label"].as_str().unwrap_or_default();
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule, enabled, run_once, kind)
             VALUES ($1, $2, $3, $4, $5, TRUE, FALSE, $6)",
        )
        .bind(user.as_ref())
        .bind(job.id.to_string())
        .bind(label)
        .bind(&job.task)
        .bind(encode_schedule(&job.schedule))
        .bind(DIGEST_KIND)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.notify_crons_changed();
        Ok(true)
    }

    /// Whether the user has a digest routine switched on.
    pub(crate) async fn digest_enabled(&self, user: &UserKey) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM crons WHERE user_id=$1 AND kind=$2 AND enabled)",
        )
        .bind(user.as_ref())
        .bind(DIGEST_KIND)
        .fetch_one(&self.pool)
        .await?)
    }

    pub(crate) async fn flag_attention(&self, user: &UserKey, item: &AttentionItem) -> Result<()> {
        sqlx::query(
            "INSERT INTO attention_items (user_id, mailbox, message_id, received_at, text)
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        )
        .bind(user.as_ref())
        .bind(item.mailbox.as_ref())
        .bind(&item.message_id)
        .bind(item.received)
        .bind(&item.text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every flagged email, oldest first, after dropping those older than `keep`.
    pub(crate) async fn attention_items(
        &self,
        user: &UserKey,
        keep: Duration,
    ) -> Result<Vec<AttentionItem>> {
        sqlx::query(
            "DELETE FROM attention_items WHERE user_id=$1
                 AND received_at < now() - make_interval(secs => $2)",
        )
        .bind(user.as_ref())
        .bind(keep.as_secs_f64())
        .execute(&self.pool)
        .await?;
        let rows = sqlx::query(
            "SELECT mailbox, message_id, received_at, text FROM attention_items
             WHERE user_id=$1 ORDER BY received_at",
        )
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        let mut items = Vec::new();

        for row in rows {
            let slug: String = row.try_get("mailbox")?;
            let Some(Feed::Mail(mailbox)) = slug.parse::<Feed>().ok() else {
                continue;
            };
            items.push(AttentionItem {
                mailbox,
                message_id: row.try_get("message_id")?,
                received: row.try_get("received_at")?,
                text: row.try_get("text")?,
            });
        }

        Ok(items)
    }

    /// Forgets flagged emails the digest dealt with.
    pub(crate) async fn clear_attention(
        &self,
        user: &UserKey,
        mailbox: Mailbox,
        ids: &[String],
    ) -> Result<()> {
        sqlx::query(
            "DELETE FROM attention_items WHERE user_id=$1 AND mailbox=$2
                 AND message_id = ANY($3)",
        )
        .bind(user.as_ref())
        .bind(mailbox.as_ref())
        .bind(ids)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/store_digest.rs"]
mod tests;
