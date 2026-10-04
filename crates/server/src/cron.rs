use std::{path::Path, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use flashmind::core::CancellationToken;
use flashmind::cron::CronListTool;
use flashmind::cron::{
    CronCreateTool, CronDeleteTool, CronEditTool, CronHandler, CronJob, CronLog, CronRegistry,
    CronRunner, CronStore, ScheduleOnceTool, TimezoneResolver,
};
use serde_json::json;
use uuid::Uuid;

use crate::app::{App, Message};
use crate::store::{CreateRun, Store};

/// Cron jobs kept in the `crons` table, all of them or one user's.
pub struct BeepCronStore {
    store: Store,
    user: Option<String>,
}

impl BeepCronStore {
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

#[async_trait]
impl CronStore for BeepCronStore {
    async fn list(&self) -> Result<Vec<CronJob>> {
        self.store.cron_jobs(self.user.as_deref()).await
    }

    async fn get(&self, id: Uuid) -> Result<Option<CronJob>> {
        self.store.cron_job(id, self.user.as_deref()).await
    }

    async fn upsert(&self, job: &CronJob) -> Result<()> {
        self.store.upsert_cron_job(job).await?;
        self.store.notify_crons_changed();
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> Result<bool> {
        let deleted = self.store.delete_cron_job(id, self.user.as_deref()).await?;
        self.store.notify_crons_changed();
        Ok(deleted)
    }
}

/// The user message a routine wakes the agent with. History reads it back
/// with [`scheduled_label`] so clients can show it as a routine, not a prompt.
fn scheduled_prompt(label: &str, task: &str) -> String {
    if label.is_empty() {
        format!("[Scheduled] {task}")
    } else {
        format!("[Scheduled: {label}] {task}")
    }
}

/// The routine's label when `content` is a [`scheduled_prompt`], empty for an
/// unlabelled routine, and `None` for anything the user wrote.
pub fn scheduled_label(content: &str) -> Option<&str> {
    if content.starts_with("[Scheduled] ") {
        return Some("");
    }
    let rest = content.strip_prefix("[Scheduled: ")?;
    let (label, _) = rest.split_once("] ")?;
    Some(label)
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
        let message = scheduled_prompt(label, &job.task);
        let msg = Message {
            session_id,
            request_id,
            message,
            cron_id: Some(job.id.to_string()),
            images: Vec::new(),
            foreground: None,
        };

        let Some(user) = self.app.user(user_id) else {
            log::warn!("cron job for an unknown user; job_id={}", job.id);
            return Ok(None);
        };
        match self.app.submit(&user, msg).await {
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

pub fn start(app: Arc<App>, log_dir: &Path) -> (CronRunner, Arc<CronRegistry>) {
    let store = Arc::new(BeepCronStore::all(app.store.clone()));
    let registry = Arc::new(CronRegistry::new(store));
    let handler = Arc::new(BeepCronHandler { app: app.clone() });
    let cancel = CancellationToken::new();

    let log_path = log_dir.join("cron.log");
    let log = Arc::new(CronLog::new(log_path));

    let tz = Arc::new(BeepTimezoneResolver { app: app.clone() });

    // The agent's tools and the API write through their own stores, whose
    // registries the runner never hears from. Pass their writes on to it.
    let changed = app.store.crons_changed();
    let reload = registry.notify();
    tokio::spawn(async move {
        loop {
            changed.notified().await;
            reload.notify_one();
        }
    });

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
    let cron_store = Arc::new(BeepCronStore::for_user(store, user));
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
    use crate::store::{TestDb, decode_schedule};
    use chrono::{DateTime, TimeDelta, Utc};
    use flashmind::cron::JobSchedule;
    use std::time::Duration;

    #[test]
    fn scheduled_prompts_read_back_their_label() {
        assert_eq!(
            scheduled_label(&scheduled_prompt("", "Water the plants")),
            Some("")
        );
        assert_eq!(
            scheduled_label(&scheduled_prompt("Plants", "Water them")),
            Some("Plants")
        );
        assert_eq!(scheduled_label("Water the plants"), None);
        assert_eq!(scheduled_label("[Scheduled]"), None);
    }

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

    #[tokio::test]
    async fn a_reminder_from_the_tools_wakes_the_scheduler() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let changed = store.crons_changed();
        let tools = CronRegistry::new(Arc::new(BeepCronStore::for_user(store, "user")));

        let at = Utc::now() + TimeDelta::minutes(1);
        tools
            .create_with_metadata(
                JobSchedule::Once(at),
                "ping".to_owned(),
                true,
                json!({"user_id": "user"}),
            )
            .await
            .expect("reminder saved");

        tokio::time::timeout(Duration::from_secs(1), changed.notified())
            .await
            .expect("saving a reminder notifies the scheduler");
    }
}
