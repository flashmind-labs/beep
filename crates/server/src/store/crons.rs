//! The user's routines as the phone lists and edits them.

use super::{Store, format_time};
use crate::{
    places::{PlaceRepeat, PlaceTrigger},
    protocol::Cron,
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

/// Whether `text` is 1 to `max` characters long.
fn within(text: &str, max: usize) -> bool {
    !text.is_empty() && text.chars().count() <= max
}

fn check_label(label: &str) -> Result<()> {
    if !within(label, 200) {
        bail!("cron label must be 1-200 characters");
    }
    Ok(())
}

fn check_prompt(prompt: &str) -> Result<()> {
    if !within(prompt, 4000) {
        bail!("cron prompt must be 1-4000 characters");
    }
    Ok(())
}

fn check_schedule(schedule: &str) -> Result<()> {
    if !within(schedule, 100) {
        bail!("cron schedule must be 1-100 characters");
    }
    Ok(())
}

/// Whether a place schedule runs only the next time, or `None` for any
/// other schedule.
fn place_once(schedule: &str) -> Option<bool> {
    PlaceTrigger::parse(schedule).map(|trigger| trigger.repeat == PlaceRepeat::Once)
}

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
        // Stored trimmed, so the generated place_id sees a place trigger.
        let schedule = schedule.trim();
        check_label(label)?;
        check_prompt(prompt)?;
        check_schedule(schedule)?;
        self.check_place_schedule(user, schedule).await?;
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule, run_once)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user.as_str())
        .bind(&id)
        .bind(label)
        .bind(prompt)
        .bind(schedule)
        .bind(place_once(schedule).unwrap_or(false))
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
        if let Some(label) = label {
            check_label(label)?;
        }
        let schedule = schedule.map(str::trim);
        if let Some(schedule) = schedule {
            check_schedule(schedule)?;
            self.check_place_schedule(user, schedule).await?;
        }
        let changed = sqlx::query(
            "UPDATE crons SET enabled=COALESCE($1, enabled), label=COALESCE($2, label),
                 schedule=COALESCE($3, schedule),
                 run_once=CASE WHEN $6 IS NOT NULL THEN $6
                     WHEN $3 IS NOT NULL AND place_id IS NOT NULL THEN FALSE
                     ELSE run_once END
             WHERE user_id=$4 AND id=$5 AND group_id IS NULL AND ($1 IS NOT NULL OR $2 IS NOT NULL OR $3 IS NOT NULL)",
        )
        .bind(enabled)
        .bind(label)
        .bind(schedule)
        .bind(user.as_str())
        .bind(id)
        .bind(schedule.and_then(place_once))
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

#[cfg(test)]
mod tests {
    use crate::{store::TestDb, user::UserKey};

    #[tokio::test]
    async fn updates_are_held_to_the_same_bounds_as_new_routines() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let id = store
            .create_cron(&alice, "Stretch", "0 9 * * *", "Remind me to stretch")
            .await
            .unwrap();

        assert!(
            store
                .update_cron(&alice, &id, None, Some(""), None)
                .await
                .is_err()
        );
        let long = "x".repeat(201);
        assert!(
            store
                .update_cron(&alice, &id, None, Some(&long), None)
                .await
                .is_err()
        );
        assert!(
            store
                .update_cron(&alice, &id, None, None, Some(""))
                .await
                .is_err()
        );
        let crons = store.list_crons(&alice).await.unwrap();
        assert_eq!(crons[0].label, "Stretch");
        assert_eq!(crons[0].schedule, "0 9 * * *");
        assert!(
            store
                .update_cron(&alice, &id, None, Some("Walk"), None)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn limits_count_characters_not_bytes() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let label = "é".repeat(200);
        store
            .create_cron(&alice, &label, "0 9 * * *", "Remind me")
            .await
            .unwrap();
        let label = "é".repeat(201);
        assert!(
            store
                .create_cron(&alice, &label, "0 9 * * *", "Remind me")
                .await
                .is_err()
        );
    }
}
