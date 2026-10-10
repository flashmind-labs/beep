//! The user's routines as the phone lists and edits them.

use super::{Store, format_time};
use crate::{protocol::Cron, user::UserKey};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

impl Store {
    pub(crate) async fn list_crons(&self, user: &UserKey) -> Result<Vec<Cron>> {
        let rows = sqlx::query(
            "SELECT id, label, prompt, schedule, enabled, created_at, run_once, last_run_at
             FROM crons WHERE user_id=$1 AND group_id IS NULL ORDER BY created_at DESC LIMIT 100",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(Cron {
                    id: r.try_get(0)?,
                    label: r.try_get(1)?,
                    prompt: r.try_get(2)?,
                    schedule: r.try_get(3)?,
                    enabled: r.try_get(4)?,
                    created_at: format_time(r.try_get(5)?),
                    run_once: r.try_get(6)?,
                    last_run_at: r.try_get::<Option<DateTime<Utc>>, _>(7)?.map(format_time),
                    next_run_at: None,
                })
            })
            .collect()
    }

    pub(crate) async fn create_cron(
        &self,
        user: &UserKey,
        label: &str,
        schedule: &str,
        prompt: &str,
    ) -> Result<String> {
        if label.is_empty() || label.len() > 200 {
            bail!("cron label must be 1–200 characters");
        }
        if prompt.is_empty() || prompt.len() > 4000 {
            bail!("cron prompt must be 1–4000 characters");
        }
        if schedule.is_empty() || schedule.len() > 100 {
            bail!("cron schedule must be 1–100 characters");
        }
        self.check_place_schedule(user, schedule).await?;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(user.as_str())
        .bind(&id)
        .bind(label)
        .bind(prompt)
        .bind(schedule)
        .execute(&self.pool)
        .await?;
        self.notify_crons_changed();
        Ok(id)
    }

    pub(crate) async fn update_cron(
        &self,
        user: &UserKey,
        id: &str,
        enabled: Option<bool>,
        label: Option<&str>,
        schedule: Option<&str>,
    ) -> Result<bool> {
        if let Some(schedule) = schedule {
            self.check_place_schedule(user, schedule).await?;
        }
        let changed = sqlx::query(
            "UPDATE crons SET enabled=COALESCE($1, enabled), label=COALESCE($2, label),
                 schedule=COALESCE($3, schedule)
             WHERE user_id=$4 AND id=$5 AND group_id IS NULL AND ($1 IS NOT NULL OR $2 IS NOT NULL OR $3 IS NOT NULL)",
        )
        .bind(enabled)
        .bind(label)
        .bind(schedule)
        .bind(user.as_str())
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        self.notify_crons_changed();
        Ok(changed > 0)
    }

    pub(crate) async fn toggle_cron(&self, user: &UserKey, id: &str) -> Result<Option<bool>> {
        let new_enabled = sqlx::query_scalar(
            "UPDATE crons SET enabled = NOT enabled WHERE user_id=$1 AND id=$2 AND group_id IS NULL RETURNING enabled",
        )
        .bind(user.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        self.notify_crons_changed();
        Ok(new_enabled)
    }

    pub(crate) async fn delete_cron(&self, user: &UserKey, id: &str) -> Result<bool> {
        let deleted =
            sqlx::query("DELETE FROM crons WHERE user_id=$1 AND id=$2 AND group_id IS NULL")
                .bind(user.as_str())
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected();
        self.notify_crons_changed();
        Ok(deleted > 0)
    }
}
