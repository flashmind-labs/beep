//! Habits and their logs.

use super::{Store, format_time};
use crate::account::user::UserKey;
use anyhow::{Result, bail};
use chrono::{Days, NaiveDate, Utc};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

/// A habit of that name is already being tracked.
#[derive(Debug, thiserror::Error)]
#[error("a habit of that name already exists")]
pub(crate) struct DuplicateHabit;

impl Store {
    pub(crate) async fn create_habit(
        &self,
        user: &UserKey,
        name: &str,
        frequency: &str,
    ) -> Result<String> {
        if name.is_empty() || name.chars().count() > 200 {
            bail!("habit name must be 1-200 characters");
        }
        let valid_freq = ["daily", "weekly", "weekdays"];
        if !valid_freq.contains(&frequency) {
            bail!(
                "invalid frequency '{}', must be one of: {}",
                frequency,
                valid_freq.join(", ")
            );
        }
        // An archived habit of the same name comes back, with its logs.
        let id: Option<String> = sqlx::query_scalar(
            "INSERT INTO habits (id, user_id, name, frequency) VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, name) DO UPDATE SET archived=FALSE, frequency=excluded.frequency
                 WHERE habits.archived
             RETURNING id",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(user.as_str())
        .bind(name)
        .bind(frequency)
        .fetch_optional(&self.pool)
        .await?;
        id.ok_or_else(|| DuplicateHabit.into())
    }

    pub(crate) async fn list_habits(&self, user: &UserKey) -> Result<Vec<Value>> {
        let rows = sqlx::query(
            "SELECT h.id, h.name, h.frequency, h.created_at,
                    (SELECT COUNT(*) FROM habit_logs l WHERE l.habit_id=h.id)
             FROM habits h
             WHERE h.user_id=$1 AND NOT h.archived
             ORDER BY h.created_at",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut result = Vec::with_capacity(rows.len());
        for r in rows {
            let habit_id: String = r.try_get(0)?;
            result.push(json!({
                "name": r.try_get::<String, _>(1)?,
                "frequency": r.try_get::<String, _>(2)?,
                "created_at": format_time(r.try_get(3)?),
                "total_logs": r.try_get::<i64, _>(4)?,
                "current_streak": self.habit_streak(&habit_id).await?,
            }));
        }
        Ok(result)
    }

    pub(crate) async fn log_habit(
        &self,
        user: &UserKey,
        habit_name: &str,
        value: f64,
        note: Option<&str>,
    ) -> Result<()> {
        let inserted = sqlx::query(
            "INSERT INTO habit_logs (id, habit_id, user_id, value, note)
             SELECT $1, id, user_id, $4, $5 FROM habits
             WHERE user_id=$2 AND name=$3 AND NOT archived",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(user.as_str())
        .bind(habit_name)
        .bind(value)
        .bind(note)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if inserted == 0 {
            bail!("habit not found: check the name and make sure it is not archived");
        }
        Ok(())
    }

    pub(crate) async fn habit_status(
        &self,
        user: &UserKey,
        habit_name: &str,
        days: i64,
    ) -> Result<Value> {
        let row = sqlx::query(
            "SELECT id, name, frequency, created_at FROM habits
             WHERE user_id=$1 AND name=$2 AND NOT archived",
        )
        .bind(user.as_str())
        .bind(habit_name)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(json!({"error": "habit not found"}));
        };
        let habit_id: String = row.try_get(0)?;
        // Today is one of the days.
        let since = Utc::now()
            .date_naive()
            .checked_sub_days(Days::new(days.saturating_sub(1).max(0).unsigned_abs()))
            .unwrap_or(NaiveDate::MIN);
        let logs = sqlx::query(
            "SELECT (logged_at AT TIME ZONE 'UTC')::date AS day, SUM(value),
                    string_agg(note, '; ')
             FROM habit_logs
             WHERE habit_id=$1 AND (logged_at AT TIME ZONE 'UTC')::date >= $2
             GROUP BY day ORDER BY day",
        )
        .bind(&habit_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| {
            Ok(json!({
                "date": r.try_get::<NaiveDate, _>(0)?.format("%Y-%m-%d").to_string(),
                "total": r.try_get::<f64, _>(1)?,
                "notes": r.try_get::<Option<String>, _>(2)?,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

        let logged_days = logs.len() as f64;
        let rate = if days > 0 {
            logged_days / days as f64
        } else {
            0.0
        };
        Ok(json!({
            "name": row.try_get::<String, _>(1)?,
            "frequency": row.try_get::<String, _>(2)?,
            "created_at": format_time(row.try_get(3)?),
            "days_queried": days,
            "logged_days": logged_days as i64,
            "completion_rate": (rate * 100.0).round() / 100.0,
            "current_streak": self.habit_streak(&habit_id).await?,
            "logs": logs,
        }))
    }

    /// Consecutive days with at least one log, going back from today.
    async fn habit_streak(&self, habit_id: &str) -> Result<i64> {
        let days: Vec<NaiveDate> = sqlx::query_scalar(
            "SELECT DISTINCT (logged_at AT TIME ZONE 'UTC')::date AS day FROM habit_logs
             WHERE habit_id=$1 ORDER BY day DESC",
        )
        .bind(habit_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(streak(&days, Utc::now().date_naive()))
    }

    pub(crate) async fn archive_habit(&self, user: &UserKey, habit_name: &str) -> Result<bool> {
        let changed = sqlx::query(
            "UPDATE habits SET archived=TRUE WHERE user_id=$1 AND name=$2 AND NOT archived",
        )
        .bind(user.as_str())
        .bind(habit_name)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed > 0)
    }
}

/// Consecutive days in `days` (distinct, newest first) ending today, or yesterday if nothing logged today.
fn streak(days: &[NaiveDate], today: NaiveDate) -> i64 {
    let Some(&newest) = days.first() else {
        return 0;
    };
    if newest != today && Some(newest) != today.pred_opt() {
        return 0;
    }
    let mut expected = newest;
    let mut streak = 0;
    for &day in days {
        if day != expected {
            break;
        }
        streak += 1;
        let Some(previous) = day.pred_opt() else {
            break;
        };
        expected = previous;
    }
    streak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streak_counts_back_from_today_or_yesterday() {
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 3, d).unwrap();
        assert_eq!(streak(&[], day(10)), 0);
        assert_eq!(streak(&[day(10), day(9), day(7)], day(10)), 2);
        assert_eq!(streak(&[day(9), day(8)], day(10)), 2);
        assert_eq!(streak(&[day(8), day(7)], day(10)), 0);
    }

    #[tokio::test]
    async fn status_covers_exactly_the_days_asked_for() {
        use crate::store::TestDb;

        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        store.create_habit(&alice, "Run", "daily").await.unwrap();
        store.log_habit(&alice, "Run", 1.0, None).await.unwrap();
        store.log_habit(&alice, "Run", 1.0, None).await.unwrap();
        sqlx::query(
            "UPDATE habit_logs SET logged_at = logged_at - interval '1 day'
             WHERE id = (SELECT id FROM habit_logs LIMIT 1)",
        )
        .execute(store.pool())
        .await
        .unwrap();

        let today = store.habit_status(&alice, "Run", 1).await.unwrap();
        assert_eq!(today["logged_days"], 1);
        assert_eq!(today["completion_rate"], 1.0);
        let two = store.habit_status(&alice, "Run", 2).await.unwrap();
        assert_eq!(two["logged_days"], 2);
        assert_eq!(two["completion_rate"], 1.0);
    }

    #[tokio::test]
    async fn names_count_characters_not_bytes() {
        use crate::store::TestDb;

        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        store
            .create_habit(&alice, &"é".repeat(200), "daily")
            .await
            .unwrap();
        assert!(
            store
                .create_habit(&alice, &"é".repeat(201), "daily")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn an_archived_habit_comes_back_by_name() {
        use crate::store::TestDb;

        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        let id = store.create_habit(&alice, "Run", "daily").await.unwrap();
        assert!(store.create_habit(&alice, "Run", "daily").await.is_err());
        assert!(store.archive_habit(&alice, "Run").await.unwrap());
        let again = store.create_habit(&alice, "Run", "weekly").await.unwrap();
        assert_eq!(again, id);
        assert_eq!(store.list_habits(&alice).await.unwrap().len(), 1);
    }
}
