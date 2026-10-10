//! The `crons` table as flashmind cron jobs, for the scheduler and the
//! cron tools. `user` limits a call to one user's own jobs; `None` sees
//! all the scheduler runs, group reminders too but not routines that start
//! at a place.

use super::Store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use flashmind::cron::{CronJob, JobSchedule};
use serde_json::json;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

pub(super) const COLUMNS: &str = "id, user_id, label, prompt, schedule, enabled, run_once, created_at, \
    last_run_at, session_id, kind, group_id";

/// A routine that wakes the agent with its prompt; the other kind is
/// [`crate::digest::DIGEST_KIND`]. A routine's kind never changes.
const PROMPT_KIND: &str = "prompt";

pub(super) fn job_from(row: &PgRow) -> Result<CronJob> {
    let id: String = row.try_get("id")?;
    let user_id: String = row.try_get("user_id")?;
    let label: String = row.try_get("label")?;
    let session_id: Option<String> = row.try_get("session_id")?;
    let kind: String = row.try_get("kind")?;
    let group_id: Option<String> = row.try_get("group_id")?;
    Ok(CronJob {
        id: id.parse().unwrap_or_else(|_| Uuid::new_v4()),
        schedule: decode_schedule(row.try_get("schedule")?),
        task: row.try_get("prompt")?,
        enabled: row.try_get("enabled")?,
        once: row.try_get("run_once")?,
        metadata: json!({"user_id": user_id, "label": label, "session_id": session_id, "kind": kind,
            "group_id": group_id}),
        created_at: row.try_get("created_at")?,
        last_run: row.try_get("last_run_at")?,
    })
}

/// Reverses [`encode_schedule`].
pub(crate) fn decode_schedule(schedule: String) -> JobSchedule {
    match DateTime::parse_from_rfc3339(&schedule) {
        Ok(at) => JobSchedule::Once(at.with_timezone(&Utc)),
        Err(_) => JobSchedule::Cron(schedule),
    }
}

/// A one-off time as RFC 3339, anything else as a cron expression.
pub(crate) fn encode_schedule(schedule: &JobSchedule) -> String {
    match schedule {
        JobSchedule::Cron(expr) => expr.clone(),
        JobSchedule::Once(at) => at.to_rfc3339(),
        JobSchedule::OnWake { .. } => "on_wake".to_owned(),
    }
}

fn meta<'a>(job: &'a CronJob, key: &str) -> Option<&'a str> {
    job.metadata.get(key).and_then(|v| v.as_str())
}

impl Store {
    pub(crate) async fn cron_jobs(&self, user: Option<&str>) -> Result<Vec<CronJob>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM crons
             WHERE ($1::text IS NULL AND place_id IS NULL) OR (user_id=$1 AND group_id IS NULL)
             ORDER BY created_at DESC LIMIT 500"
        ))
        .bind(user)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(job_from).collect()
    }

    pub(crate) async fn cron_job(&self, id: Uuid, user: Option<&str>) -> Result<Option<CronJob>> {
        let row = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM crons
             WHERE id=$1 AND ($2::text IS NULL OR (user_id=$2 AND group_id IS NULL))"
        ))
        .bind(id.to_string())
        .bind(user)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(job_from).transpose()
    }

    pub(crate) async fn upsert_cron_job(&self, job: &CronJob) -> Result<()> {
        let schedule = encode_schedule(&job.schedule);
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule, enabled, run_once,
                 created_at, last_run_at, session_id, kind, group_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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
        .bind(meta(job, "kind").unwrap_or(PROMPT_KIND))
        .bind(meta(job, "group_id"))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The chat `id`'s runs reply in: the one it already has, or `session`,
    /// which it keeps from then on. `None` when the job is gone.
    pub(crate) async fn claim_cron_session(
        &self,
        id: Uuid,
        session: Uuid,
    ) -> Result<Option<String>> {
        let row = sqlx::query(
            "UPDATE crons SET session_id=COALESCE(session_id, $2) WHERE id=$1 RETURNING session_id",
        )
        .bind(id.to_string())
        .bind(session.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.try_get("session_id")).transpose()?)
    }

    pub(crate) async fn delete_cron_job(&self, id: Uuid, user: Option<&str>) -> Result<bool> {
        let deleted = sqlx::query(
            "DELETE FROM crons
                 WHERE id=$1 AND ($2::text IS NULL OR (user_id=$2 AND group_id IS NULL))",
        )
        .bind(id.to_string())
        .bind(user)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(deleted > 0)
    }

    /// The reminders set in group `group`, oldest first.
    pub(crate) async fn group_reminders(&self, group: &str) -> Result<Vec<CronJob>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM crons WHERE group_id=$1 ORDER BY created_at LIMIT 100"
        ))
        .bind(group)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(job_from).collect()
    }

    /// Deletes reminder `id` from group `group`; false when it is not there.
    pub(crate) async fn delete_group_reminder(&self, group: &str, id: Uuid) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM crons WHERE id=$1 AND group_id=$2")
            .bind(id.to_string())
            .bind(group)
            .execute(&self.pool)
            .await?
            .rows_affected();
        self.notify_crons_changed();
        Ok(deleted > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::{store::TestDb, user::UserKey};
    use chrono::Utc;
    use flashmind::cron::{CronJob, JobSchedule};
    use serde_json::json;
    use uuid::Uuid;

    fn job(metadata: serde_json::Value) -> CronJob {
        CronJob {
            id: Uuid::new_v4(),
            schedule: JobSchedule::Cron("0 9 * * *".to_owned()),
            task: "Check-out at 11".to_owned(),
            enabled: true,
            once: false,
            metadata,
            created_at: Utc::now(),
            last_run: None,
        }
    }

    #[tokio::test]
    async fn group_reminders_stay_out_of_the_members_routines() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store opens");
        let alice = UserKey::for_test("alice");
        store
            .create_group(&alice, "g1", "Trip", "Ali")
            .await
            .expect("group made");
        let own = job(json!({"user_id": "alice", "label": "Own"}));
        let group = job(json!({"user_id": "alice", "label": "Trip", "group_id": "g1"}));
        store.upsert_cron_job(&own).await.expect("routine saved");
        store.upsert_cron_job(&group).await.expect("reminder saved");

        let mine = store.cron_jobs(Some("alice")).await.expect("jobs load");
        assert_eq!(mine.iter().map(|j| j.id).collect::<Vec<_>>(), [own.id]);
        assert_eq!(
            store.list_crons(&alice).await.expect("routines load").len(),
            1
        );
        assert!(
            !store
                .delete_cron(&alice, &group.id.to_string())
                .await
                .expect("delete runs")
        );

        let all = store.cron_jobs(None).await.expect("jobs load");
        let fired = all
            .iter()
            .find(|j| j.id == group.id)
            .expect("scheduler sees it");
        assert_eq!(fired.metadata["group_id"], "g1");
        assert_eq!(
            store
                .group_reminders("g1")
                .await
                .expect("reminders load")
                .len(),
            1
        );

        assert!(
            !store
                .delete_group_reminder("g2", group.id)
                .await
                .expect("delete runs")
        );
        assert!(
            store
                .delete_group_reminder("g1", group.id)
                .await
                .expect("delete runs")
        );
        assert!(
            store
                .group_reminders("g1")
                .await
                .expect("reminders load")
                .is_empty()
        );
    }
}
