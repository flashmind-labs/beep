//! The phone's last position, and the user's timezone.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;

/// The phone's last reported position.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    /// Radius in meters.
    pub accuracy: f64,
    pub timezone: String,
    /// Reverse-geocoded name, such as "Shinjuku, Tokyo, Japan".
    pub place: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Store {
    /// Saves the phone's location and the timezone it was in.
    pub(crate) async fn update_location(&self, user: &UserKey, fix: &Location) -> Result<()> {
        sqlx::query(
            "INSERT INTO locations (user_id, latitude, longitude, accuracy, timezone, place, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id) DO UPDATE SET latitude=excluded.latitude,
                 longitude=excluded.longitude, accuracy=excluded.accuracy,
                 timezone=excluded.timezone, place=excluded.place, updated_at=excluded.updated_at",
        )
        .bind(user.as_str())
        .bind(fix.latitude)
        .bind(fix.longitude)
        .bind(fix.accuracy)
        .bind(&fix.timezone)
        .bind(&fix.place)
        .bind(fix.updated_at)
        .execute(&self.pool)
        .await?;
        self.set_timezone(user, &fix.timezone).await
    }

    /// Saves the user's timezone. A new one reloads the scheduler, since
    /// recurring jobs run in the user's local time.
    pub(crate) async fn set_timezone(&self, user: &UserKey, timezone: &str) -> Result<()> {
        let changed = sqlx::query(
            "UPDATE users SET timezone=$2 WHERE id=$1 AND timezone IS DISTINCT FROM $2",
        )
        .bind(user.as_str())
        .bind(timezone)
        .execute(&self.pool)
        .await?
        .rows_affected()
            > 0;
        if changed {
            self.notify_crons_changed();
        }
        Ok(())
    }

    pub(crate) async fn get_location(&self, user: &UserKey) -> Result<Option<Location>> {
        let row = sqlx::query(
            "SELECT latitude, longitude, accuracy, timezone, place, updated_at FROM locations
             WHERE user_id=$1",
        )
        .bind(user.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| {
            Ok(Location {
                latitude: r.try_get(0)?,
                longitude: r.try_get(1)?,
                accuracy: r.try_get(2)?,
                timezone: r.try_get(3)?,
                place: r.try_get(4)?,
                updated_at: r.try_get(5)?,
            })
        })
        .transpose()
    }

    pub(crate) async fn get_timezone(&self, user: &UserKey) -> Result<String> {
        let timezone: Option<String> = sqlx::query_scalar("SELECT timezone FROM users WHERE id=$1")
            .bind(user.as_str())
            .fetch_optional(&self.pool)
            .await?
            .flatten();
        Ok(timezone.unwrap_or_else(|| "UTC".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;
    use std::time::Duration;

    #[tokio::test]
    async fn a_new_timezone_reloads_the_scheduler() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let changed = store.crons_changed();
        let user = UserKey::for_test("alice");
        let at = |timezone: &str| Location {
            latitude: 1.0,
            longitude: 2.0,
            accuracy: 10.0,
            timezone: timezone.to_owned(),
            place: None,
            updated_at: Utc::now(),
        };
        let reloads = || async {
            tokio::time::timeout(Duration::from_millis(200), changed.notified())
                .await
                .is_ok()
        };

        store
            .update_location(&user, &at("Europe/Madrid"))
            .await
            .unwrap();
        assert!(reloads().await, "the first timezone reloads");
        store
            .update_location(&user, &at("Europe/Madrid"))
            .await
            .unwrap();
        assert!(!reloads().await, "the same timezone does not");
        store
            .update_location(&user, &at("Asia/Tokyo"))
            .await
            .unwrap();
        assert!(reloads().await, "a new timezone does");
    }

    #[tokio::test]
    async fn the_timezone_is_kept_without_a_location() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");

        assert_eq!(store.get_timezone(&user).await.unwrap(), "UTC");
        store.set_timezone(&user, "Europe/Madrid").await.unwrap();
        assert_eq!(store.get_timezone(&user).await.unwrap(), "Europe/Madrid");
        assert_eq!(store.get_location(&user).await.unwrap(), None);
    }
}
