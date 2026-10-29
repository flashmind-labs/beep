//! What decides when a scope of the user's memories is curated: when it was
//! last curated, how big it was then, and the signals counted since.

use super::Scope;
use crate::{
    memories::{ScopeState, Signal},
    store::Store,
    user::UserKey,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;
use std::collections::HashMap;

/// The `scope` column: `Scope::stored`, empty for the user.
fn column(scope: &Scope) -> String {
    scope.stored().unwrap_or_default()
}

impl Store {
    /// The curation state of each scope of the user's memories that has one.
    pub(crate) async fn curation_states(
        &self,
        user: &UserKey,
    ) -> Result<HashMap<Scope, ScopeState>> {
        let rows = sqlx::query(
            "SELECT scope, curated_at, size, close, crowded FROM memory_curation WHERE user_id = $1",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut states = HashMap::new();
        for row in rows {
            let scope: String = row.try_get(0)?;
            let curated_at: Option<DateTime<Utc>> = row.try_get(1)?;
            let scope = Scope::from_stored(Some(scope.as_str()).filter(|s| !s.is_empty()));
            states.insert(
                scope,
                ScopeState {
                    curated_at: curated_at.map(|at| at.naive_utc()),
                    size: usize::try_from(row.try_get::<i32, _>(2)?)?,
                    close: u32::try_from(row.try_get::<i32, _>(3)?)?,
                    crowded: u32::try_from(row.try_get::<i32, _>(4)?)?,
                },
            );
        }
        Ok(states)
    }

    /// Counts one `signal` for the user's memories in `scope`.
    pub(crate) async fn note_signal(
        &self,
        user: &UserKey,
        scope: &Scope,
        signal: Signal,
    ) -> Result<()> {
        let query = match signal {
            Signal::Close => {
                "INSERT INTO memory_curation (user_id, scope, close) VALUES ($1, $2, 1)
                 ON CONFLICT (user_id, scope) DO UPDATE SET close = memory_curation.close + 1"
            }
            Signal::Crowded => {
                "INSERT INTO memory_curation (user_id, scope, crowded) VALUES ($1, $2, 1)
                 ON CONFLICT (user_id, scope) DO UPDATE SET crowded = memory_curation.crowded + 1"
            }
        };
        sqlx::query(query)
            .bind(user.as_str())
            .bind(column(scope))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records that the user's memories in `scope` were just curated and
    /// now number `size`, clearing the signals `counted` held when curation
    /// started. Signals noted while it ran stay for the next one.
    pub(crate) async fn mark_curated(
        &self,
        user: &UserKey,
        scope: &Scope,
        size: usize,
        counted: &ScopeState,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO memory_curation (user_id, scope, curated_at, size) VALUES ($1, $2, now(), $3)
             ON CONFLICT (user_id, scope) DO UPDATE
             SET curated_at = now(), size = excluded.size,
                 close = GREATEST(memory_curation.close - $4, 0),
                 crowded = GREATEST(memory_curation.crowded - $5, 0)",
        )
        .bind(user.as_str())
        .bind(column(scope))
        .bind(i32::try_from(size)?)
        .bind(i32::try_from(counted.close)?)
        .bind(i32::try_from(counted.crowded)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TestDb;

    #[tokio::test]
    async fn signals_noted_during_curation_are_kept() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let alice = UserKey::for_test("alice");
        for _ in 0..2 {
            store
                .note_signal(&alice, &Scope::User, Signal::Close)
                .await
                .unwrap();
        }
        let counted = store.curation_states(&alice).await.unwrap()[&Scope::User].clone();
        // Noted while curation ran.
        store
            .note_signal(&alice, &Scope::User, Signal::Close)
            .await
            .unwrap();
        store
            .mark_curated(&alice, &Scope::User, 5, &counted)
            .await
            .unwrap();
        let state = &store.curation_states(&alice).await.unwrap()[&Scope::User];
        assert_eq!(state.close, 1);
        assert_eq!(state.crowded, 0);
        assert_eq!(state.size, 5);
    }
}
