use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flashmind::core::CancellationToken;
use flashmind::cron::CronListTool;
use flashmind::cron::{
    CronCreateTool, CronDeleteTool, CronEditTool, CronHandler, CronJob, CronLog, CronRegistry,
    CronRunner, CronStore, JobSchedule, ScheduleOnceTool, TimezoneResolver,
};
use rusqlite::{OptionalExtension, params};
use serde_json::json;
use uuid::Uuid;

use crate::app::{App, Message};
use crate::store::{CreateRun, Store};

pub struct SqliteCronStore {
    store: Store,
    user: Option<String>,
}

impl SqliteCronStore {
    pub fn all(store: Store) -> Self {
        Self { store, user: None }
    }

    pub fn for_user(store: Store, user: &str) -> Self {
        Self {
            store,
            user: Some(user.to_owned()),
        }
    }
}

fn row_to_job(r: &rusqlite::Row) -> rusqlite::Result<CronJob> {
    let id_str: String = r.get("id")?;
    let user_id: String = r.get("user_id")?;
    let label: String = r.get("label")?;
    let prompt: String = r.get("prompt")?;
    let schedule: String = r.get("schedule")?;
    let enabled: bool = r.get("enabled")?;
    let run_once: bool = r.get("run_once")?;
    let created_at: String = r.get("created_at")?;
    let last_run_at: Option<String> = r.get("last_run_at")?;

    let id = id_str.parse::<Uuid>().unwrap_or_else(|_| Uuid::new_v4());
    let created = DateTime::parse_from_rfc3339(&created_at)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let last_run = last_run_at.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    });

    Ok(CronJob {
        id,
        schedule: decode_schedule(schedule),
        task: prompt,
        enabled,
        once: run_once,
        metadata: json!({"user_id": user_id, "label": label}),
        created_at: created,
        last_run,
    })
}

/// Reverses the encoding in `upsert`: a one-off time is stored as RFC 3339, anything else is
/// a cron expression.
fn decode_schedule(schedule: String) -> JobSchedule {
    match DateTime::parse_from_rfc3339(&schedule) {
        Ok(at) => JobSchedule::Once(at.with_timezone(&Utc)),
        Err(_) => JobSchedule::Cron(schedule),
    }
}

const SELECT_COLS: &str =
    "id, user_id, label, prompt, schedule, enabled, run_once, created_at, last_run_at";

#[async_trait]
impl CronStore for SqliteCronStore {
    async fn list(&self) -> Result<Vec<CronJob>> {
        let user = self.user.clone();
        Ok(self
            .store
            .raw()
            .call::<_, _, rusqlite::Error>(move |db| {
                if let Some(ref u) = user {
                    let mut stmt = db.prepare(&format!(
                        "SELECT {SELECT_COLS} FROM crons WHERE user_id=? ORDER BY created_at DESC LIMIT 200"
                    ))?;
                    stmt.query_map([u], row_to_job)?
                        .collect::<rusqlite::Result<Vec<_>>>()
                } else {
                    let mut stmt = db.prepare(&format!(
                        "SELECT {SELECT_COLS} FROM crons ORDER BY created_at DESC LIMIT 500"
                    ))?;
                    stmt.query_map([], row_to_job)?
                        .collect::<rusqlite::Result<Vec<_>>>()
                }
            })
            .await?)
    }

    async fn get(&self, id: Uuid) -> Result<Option<CronJob>> {
        let id_str = id.to_string();
        let user = self.user.clone();
        Ok(self
            .store
            .raw()
            .call::<_, _, rusqlite::Error>(move |db| {
                // A user's store only sees that user's jobs.
                let sql = format!(
                    "SELECT {SELECT_COLS} FROM crons WHERE id=?1 AND (?2 IS NULL OR user_id=?2)"
                );
                let mut stmt = db.prepare(&sql)?;
                let job = stmt
                    .query_row(params![id_str, user], row_to_job)
                    .optional()?;
                Ok(job)
            })
            .await?)
    }

    async fn upsert(&self, job: &CronJob) -> Result<()> {
        let id = job.id.to_string();
        let user_id = job
            .metadata
            .get("user_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_owned();
        let label = job
            .metadata
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let schedule = match &job.schedule {
            JobSchedule::Cron(expr) => expr.clone(),
            JobSchedule::Once(dt) => dt.to_rfc3339(),
            JobSchedule::OnWake { .. } => "on_wake".to_owned(),
        };
        let prompt = job.task.clone();
        let enabled = job.enabled;
        let run_once = job.once;
        let created_at = job.created_at.to_rfc3339();
        let last_run_at = job.last_run.map(|dt| dt.to_rfc3339());

        self.store
            .raw()
            .call::<_, _, rusqlite::Error>(move |db| {
                db.execute(
                    "INSERT INTO crons (user_id, id, label, prompt, schedule, enabled, run_once, created_at, last_run_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(user_id, id) DO UPDATE SET
                       label=?3, prompt=?4, schedule=?5, enabled=?6, run_once=?7, last_run_at=?9",
                    params![user_id, id, label, prompt, schedule, enabled, run_once, created_at, last_run_at],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<bool> {
        let id_str = id.to_string();
        let user = self.user.clone();
        Ok(self
            .store
            .raw()
            .call::<_, _, rusqlite::Error>(move |db| {
                let deleted = db.execute(
                    "DELETE FROM crons WHERE id=?1 AND (?2 IS NULL OR user_id=?2)",
                    params![id_str, user],
                )?;
                Ok(deleted > 0)
            })
            .await?)
    }
}

struct BeepCronHandler {
    app: Arc<App>,
}

#[async_trait]
impl CronHandler for BeepCronHandler {
    async fn execute(self: Arc<Self>, job: &CronJob) -> Result<Option<String>> {
        let user_id = job
            .metadata
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("job missing user_id in metadata"))?;
        let label = job
            .metadata
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let session_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let message = format!("[Scheduled: {label}] {}", job.task);
        let msg = Message {
            session_id,
            request_id,
            message,
            cron_id: Some(job.id.to_string()),
            image: None,
        };

        let Some(user) = self.app.user(user_id) else {
            log::warn!("cron job for an unknown user; job_id={}", job.id);
            return Ok(None);
        };
        match self.app.submit(user, msg).await {
            Ok(CreateRun::Created(run)) => Ok(Some(run.id)),
            Ok(_) => {
                log::debug!("cron run not created (conflict); job_id={}", job.id);
                Ok(None)
            }
            Err(e) => {
                log::error!("failed to submit cron run: {e}; job_id={}", job.id);
                Err(e)
            }
        }
    }
}

struct BeepTimezoneResolver {
    app: Arc<App>,
}

#[async_trait]
impl TimezoneResolver for BeepTimezoneResolver {
    async fn resolve(&self, job: &CronJob) -> Option<chrono_tz::Tz> {
        let user_id = job.metadata.get("user_id")?.as_str()?;
        let tz_name = self.app.user(user_id)?.get_timezone().await.ok()?;
        tz_name.parse().ok()
    }
}

pub fn start(app: Arc<App>, db_path: &std::path::Path) -> (CronRunner, Arc<CronRegistry>) {
    let store = Arc::new(SqliteCronStore::all(app.store.clone()));
    let registry = Arc::new(CronRegistry::new(store));
    let handler = Arc::new(BeepCronHandler { app: app.clone() });
    let cancel = CancellationToken::new();

    let log_path = db_path.with_extension("cron.log");
    let log = Arc::new(CronLog::new(log_path));

    let tz = Arc::new(BeepTimezoneResolver { app: app.clone() });

    let runner = CronRunner::new(registry.clone(), handler, cancel)
        .with_log(log)
        .with_timezone_resolver(tz);

    (runner, registry)
}

pub fn register_tools(
    registry: &mut flashmind::types::ToolRegistry,
    store: Store,
    user: &str,
    cron_log: Option<Arc<CronLog>>,
) {
    let cron_store = Arc::new(SqliteCronStore::for_user(store, user));
    let cron_registry = Arc::new(CronRegistry::new(cron_store));
    let user_meta = json!({"user_id": user});

    registry.register(Arc::new(CronCreateTool::with_metadata(
        cron_registry.clone(),
        user_meta.clone(),
    )));
    registry.register(Arc::new(CronListTool::new(cron_registry.clone())));
    registry.register(Arc::new(CronEditTool::new(cron_registry.clone())));
    registry.register(Arc::new(CronDeleteTool::new(cron_registry.clone())));
    registry.register(Arc::new(ScheduleOnceTool::with_metadata(
        cron_registry,
        user_meta,
    )));

    if let Some(log) = cron_log {
        registry.register(Arc::new(flashmind::cron::CronHistoryTool::new(log)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_off_time_reads_back_as_once() {
        let at = "2026-10-04T04:07:00+00:00"
            .parse::<DateTime<Utc>>()
            .expect("valid time");
        assert!(matches!(decode_schedule(at.to_rfc3339()), JobSchedule::Once(t) if t == at));
        assert!(
            matches!(decode_schedule("0 8 * * *".to_owned()), JobSchedule::Cron(e) if e == "0 8 * * *")
        );
    }
}
