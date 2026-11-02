//! The versions of each user's instructions for their agents.

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};

/// One saved version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstructionsRow {
    pub(crate) version: u32,
    pub(crate) text: String,
    pub(crate) note: String,
    pub(crate) saved_at: DateTime<Utc>,
}

/// What [`Store::save_instructions`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Saved {
    Made(InstructionsRow),
    /// A newer version than the base was saved since: this one.
    Stale(u32),
}

/// The most versions listed.
const LISTED: i64 = 50;

fn row_from(row: &PgRow) -> Result<InstructionsRow> {
    let version: i32 = row.try_get(0)?;
    Ok(InstructionsRow {
        version: u32::try_from(version)?,
        text: row.try_get(1)?,
        note: row.try_get(2)?,
        saved_at: row.try_get(3)?,
    })
}

impl Store {
    /// The newest `limit` versions, newest first.
    pub(crate) async fn instructions_versions(
        &self,
        user: &UserKey,
        limit: i64,
    ) -> Result<Vec<InstructionsRow>> {
        let rows = sqlx::query(
            "SELECT version, text, note, saved_at FROM instructions_versions
             WHERE user_id=$1 ORDER BY version DESC LIMIT $2",
        )
        .bind(user.as_str())
        .bind(limit.min(LISTED))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_from).collect()
    }

    /// Saves `text` as the version after `base`, unless a newer one than
    /// `base` is there.
    pub(crate) async fn save_instructions(
        &self,
        user: &UserKey,
        base: Option<u32>,
        text: &str,
        note: &str,
    ) -> Result<Saved> {
        let mut tx = self.pool.begin().await?;
        // One save at a time per user, so two never take the same number.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('instructions:' || $1))")
            .bind(user.as_str())
            .execute(&mut *tx)
            .await?;
        let newest: Option<i32> =
            sqlx::query_scalar("SELECT MAX(version) FROM instructions_versions WHERE user_id=$1")
                .bind(user.as_str())
                .fetch_one(&mut *tx)
                .await?;
        let newest = newest.map(u32::try_from).transpose()?;
        if newest != base {
            return Ok(Saved::Stale(newest.unwrap_or_default()));
        }
        let next = i32::try_from(newest.unwrap_or_default())?
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("too many versions"))?;
        let row = sqlx::query(
            "INSERT INTO instructions_versions (user_id, version, text, note)
             VALUES ($1, $2, $3, $4) RETURNING version, text, note, saved_at",
        )
        .bind(user.as_str())
        .bind(next)
        .bind(text)
        .bind(note)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Saved::Made(row_from(&row)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;

    #[tokio::test]
    async fn versions_count_up_and_a_stale_base_is_refused() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        assert!(
            store
                .instructions_versions(&user, 1)
                .await
                .unwrap()
                .is_empty()
        );
        let Saved::Made(first) = store
            .save_instructions(&user, None, "- Write plainly.", "")
            .await
            .unwrap()
        else {
            panic!("the first save is made");
        };
        assert_eq!(first.version, 1);
        assert_eq!(
            store
                .save_instructions(&user, None, "other", "")
                .await
                .unwrap(),
            Saved::Stale(1)
        );
        let Saved::Made(second) = store
            .save_instructions(&user, Some(1), "- Keep it short.", "Restored version 1")
            .await
            .unwrap()
        else {
            panic!("a save on the newest is made");
        };
        assert_eq!(second.version, 2);
        let listed = store.instructions_versions(&user, 50).await.unwrap();
        assert_eq!(listed.iter().map(|v| v.version).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(listed[0].note, "Restored version 1");
        let bob = UserKey::for_test("bob");
        assert!(
            store
                .instructions_versions(&bob, 50)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
