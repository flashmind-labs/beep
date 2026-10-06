//! The phone's last position and the timezone it implies.

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
    /// Saves the phone's location. A new timezone reloads the scheduler, since
    /// recurring jobs run in the user's local time.
    pub(crate) async fn update_location(&self, user: &UserKey, fix: &Location) -> Result<()> {
        let previous: Option<String> = sqlx::query_scalar(
            "WITH old AS (SELECT timezone FROM locations WHERE user_id=$1)
             INSERT INTO locations (user_id, latitude, longitude, accuracy, timezone, place, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (user_id) DO UPDATE SET latitude=excluded.latitude,
                 longitude=excluded.longitude, accuracy=excluded.accuracy,
                 timezone=excluded.timezone, place=excluded.place, updated_at=excluded.updated_at
             RETURNING (SELECT timezone FROM old)",
        )
        .bind(user.as_str())
        .bind(fix.latitude)
        .bind(fix.longitude)
        .bind(fix.accuracy)
        .bind(&fix.timezone)
        .bind(&fix.place)
        .bind(fix.updated_at)
        .fetch_one(&self.pool)
        .await?;
        if previous.as_deref() != Some(fix.timezone.as_str()) {
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
        let timezone: Option<String> =
            sqlx::query_scalar("SELECT timezone FROM locations WHERE user_id=$1")
                .bind(user.as_str())
                .fetch_optional(&self.pool)
                .await?;
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
}
