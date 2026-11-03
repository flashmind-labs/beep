//! State and notes of the one-time profile pass (see `crate::profile`).

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::Row;

/// One thing the profile pass noticed about the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileNote {
    /// The day of the email or event it came from, when it has one.
    pub date: Option<NaiveDate>,
    pub text: String,
}

/// How far the read of one source got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProfileRead {
    /// Mail before this is still to read; `None` before the first year.
    pub read_before: Option<DateTime<Utc>>,
    pub finished: bool,
}

impl Store {
    pub(crate) async fn profile_read(&self, user: &UserKey, source: &str) -> Result<ProfileRead> {
        let row = sqlx::query(
            "SELECT read_before, finished_at FROM profile_reads WHERE user_id=$1 AND source=$2",
        )
        .bind(user.as_str())
        .bind(source)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(ProfileRead::default());
        };
        let finished: Option<DateTime<Utc>> = row.try_get("finished_at")?;
        Ok(ProfileRead {
            read_before: row.try_get("read_before")?,
            finished: finished.is_some(),
        })
    }

    /// Saves `notes` and moves the read of `source` back to `read_before`, together.
    pub(crate) async fn add_profile_notes(
        &self,
        user: &UserKey,
        source: &str,
        notes: &[ProfileNote],
        read_before: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let dates: Vec<Option<NaiveDate>> = notes.iter().map(|note| note.date).collect();
        let texts: Vec<&str> = notes.iter().map(|note| note.text.as_str()).collect();
        sqlx::query(
            "INSERT INTO profile_notes (user_id, source, noted_on, note)
             SELECT $1, $2, * FROM UNNEST($3::date[], $4::text[])",
        )
        .bind(user.as_str())
        .bind(source)
        .bind(&dates)
        .bind(&texts)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO profile_reads (user_id, source, read_before) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, source) DO UPDATE SET read_before=excluded.read_before",
        )
        .bind(user.as_str())
        .bind(source)
        .bind(read_before)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn finish_profile_read(&self, user: &UserKey, source: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO profile_reads (user_id, source, finished_at) VALUES ($1, $2, now())
             ON CONFLICT (user_id, source) DO UPDATE SET finished_at=now()",
        )
        .bind(user.as_str())
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Forgets the read of `source` and its notes, so the next read starts over.
    pub(crate) async fn reset_profile_read(&self, user: &UserKey, source: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for table in ["profile_notes", "profile_reads"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE user_id=$1 AND source=$2"
            ))
            .bind(user.as_str())
            .bind(source)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Every note about the user, undated ones first, then oldest first.
    pub(crate) async fn profile_notes(&self, user: &UserKey) -> Result<Vec<ProfileNote>> {
        let rows = sqlx::query(
            "SELECT noted_on, note FROM profile_notes WHERE user_id=$1
             ORDER BY noted_on NULLS FIRST, id",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ProfileNote {
                    date: row.try_get("noted_on")?,
                    text: row.try_get("note")?,
                })
            })
            .collect()
    }
}
