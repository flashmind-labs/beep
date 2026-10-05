//! The `crons` table as flashmind cron jobs, for the scheduler and the
//! cron tools. `user` limits a call to one user's jobs; `None` sees all.

use super::Store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::cron::{CronJob, JobSchedule};
use serde_json::json;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

const COLUMNS: &str =
    "id, user_id, label, prompt, schedule, enabled, run_once, created_at, last_run_at, session_id";

fn job_from(row: &PgRow) -> Result<CronJob> {
    let id: String = row.try_get("id")?;
    let user_id: String = row.try_get("user_id")?;
    let label: String = row.try_get("label")?;
    let session_id: Option<String> = row.try_get("session_id")?;
    Ok(CronJob {
        id: id.parse().unwrap_or_else(|_| Uuid::new_v4()),
        schedule: decode_schedule(row.try_get("schedule")?),
        task: row.try_get("prompt")?,
        enabled: row.try_get("enabled")?,
        once: row.try_get("run_once")?,
        metadata: json!({"user_id": user_id, "label": label, "session_id": session_id}),
        created_at: row.try_get("created_at")?,
        last_run: row.try_get("last_run_at")?,
    })
}

/// Reverses the encoding in `upsert_cron_job`: a one-off time is stored as
/// RFC 3339, anything else is a cron expression.
pub(crate) fn decode_schedule(schedule: String) -> JobSchedule {
    match DateTime::parse_from_rfc3339(&schedule) {
        Ok(at) => JobSchedule::Once(at.with_timezone(&Utc)),
        Err(_) => JobSchedule::Cron(schedule),
    }
}

fn meta<'a>(job: &'a CronJob, key: &str) -> Option<&'a str> {
    job.metadata.get(key).and_then(|v| v.as_str())
}

impl Store {
    pub(crate) async fn cron_jobs(&self, user: Option<&str>) -> Result<Vec<CronJob>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM crons WHERE $1::text IS NULL OR user_id=$1
             ORDER BY created_at DESC LIMIT 500"
        ))
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(job_from).collect()
    }

    pub(crate) async fn cron_job(&self, id: Uuid, user: Option<&str>) -> Result<Option<CronJob>> {
        let row = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM crons WHERE id=$1 AND ($2::text IS NULL OR user_id=$2)"
        ))
        .bind(id.to_string())
        .bind(user)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(job_from).transpose()
    }

    pub(crate) async fn upsert_cron_job(&self, job: &CronJob) -> Result<()> {
        let schedule = match &job.schedule {
            JobSchedule::Cron(expr) => expr.clone(),
            JobSchedule::Once(at) => at.to_rfc3339(),
            JobSchedule::OnWake { .. } => "on_wake".to_owned(),
        };
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule, enabled, run_once,
                 created_at, last_run_at, session_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (user_id, id) DO UPDATE SET label=$3, prompt=$4, schedule=$5,
                 enabled=$6, run_once=$7, last_run_at=$9,
                 session_id=COALESCE($10, crons.session_id)",
        )
        .bind(meta(job, "user_id").unwrap_or("unknown"))
        .bind(job.id.to_string())
        .bind(meta(job, "label").unwrap_or(""))
        .bind(&job.task)
        .bind(schedule)
        .bind(job.enabled)
        .bind(job.once)
        .bind(job.created_at)
        .bind(job.last_run)
        .bind(meta(job, "session_id"))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_cron_job(&self, id: Uuid, user: Option<&str>) -> Result<bool> {
        let deleted =
            sqlx::query("DELETE FROM crons WHERE id=$1 AND ($2::text IS NULL OR user_id=$2)")
                .bind(id.to_string())
                .bind(user)
                .execute(&self.pool)
                .await?
                .rows_affected();
        Ok(deleted > 0)
    }
}
