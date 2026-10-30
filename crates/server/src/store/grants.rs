//! Grants: standing permissions for one routine, and the calls they let
//! through (see `crate::grants`).

use super::Store;
use crate::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// A grant the user just approved.
pub(crate) struct NewGrant<'a> {
    pub(crate) routine: Uuid,
    pub(crate) tool: &'a str,
    pub(crate) constraints: Value,
    pub(crate) per_run: u32,
    pub(crate) shown: Value,
    pub(crate) call_id: &'a str,
    pub(crate) expires_at: DateTime<Utc>,
}

/// A grant still in force: not revoked, not expired, its routine still
/// there.
#[derive(Debug, Clone)]
pub(crate) struct GrantRow {
    pub(crate) id: Uuid,
    pub(crate) routine: String,
    /// The routine's label, or its prompt when it has none.
    pub(crate) routine_name: String,
    pub(crate) tool: String,
    pub(crate) constraints: Value,
    pub(crate) per_run: u32,
    pub(crate) shown: Value,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) last_used_at: Option<DateTime<Utc>>,
}

const LIVE: &str =
    "SELECT g.id, g.routine_id, COALESCE(NULLIF(c.label, ''), c.prompt) AS routine_name,
        g.tool, g.constraints, g.per_run, g.shown, g.expires_at, g.created_at,
        (SELECT max(u.used_at) FROM grant_uses u WHERE u.grant_id = g.id) AS last_used_at
    FROM grants g JOIN crons c ON c.user_id = g.user_id AND c.id = g.routine_id
    WHERE g.user_id = $1 AND g.revoked_at IS NULL AND g.expires_at > now()";

fn grant_from(row: &PgRow) -> Result<GrantRow> {
    let per_run: i32 = row.try_get("per_run")?;
    Ok(GrantRow {
        id: row.try_get("id")?,
        routine: row.try_get("routine_id")?,
        routine_name: row.try_get("routine_name")?,
        tool: row.try_get("tool")?,
        constraints: row.try_get("constraints")?,
        per_run: u32::try_from(per_run)?,
        shown: row.try_get("shown")?,
        expires_at: row.try_get("expires_at")?,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
    })
}

impl Store {
    /// Saves an approved grant and returns its id. Saving the same call's
    /// grant again returns the first one's id and changes nothing.
    pub(crate) async fn create_grant(&self, user: &UserKey, grant: NewGrant<'_>) -> Result<Uuid> {
        let per_run = i32::try_from(grant.per_run)?;
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO grants (id, user_id, routine_id, tool, constraints, per_run, shown,
                 call_id, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (user_id, call_id) DO NOTHING RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(user.as_str())
        .bind(grant.routine.to_string())
        .bind(grant.tool)
        .bind(&grant.constraints)
        .bind(per_run)
        .bind(&grant.shown)
        .bind(grant.call_id)
        .bind(grant.expires_at)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(id) = inserted {
            return Ok(id);
        }
        Ok(
            sqlx::query_scalar("SELECT id FROM grants WHERE user_id=$1 AND call_id=$2")
                .bind(user.as_str())
                .bind(grant.call_id)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// The user's grants in force, newest first.
    pub(crate) async fn live_grants(&self, user: &UserKey) -> Result<Vec<GrantRow>> {
        let rows = sqlx::query(&format!("{LIVE} ORDER BY g.created_at DESC LIMIT 200"))
            .bind(user.as_str())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(grant_from).collect()
    }

    /// The grants in force for calls of `tool` in runs of `routine`.
    pub(crate) async fn routine_grants(
        &self,
        user: &UserKey,
        routine: Uuid,
        tool: &str,
    ) -> Result<Vec<GrantRow>> {
        let rows = sqlx::query(&format!(
            "{LIVE} AND g.routine_id = $2 AND g.tool = $3 ORDER BY g.created_at"
        ))
        .bind(user.as_str())
        .bind(routine.to_string())
        .bind(tool)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(grant_from).collect()
    }

    /// Records call `call_id` of run `run` as a use of grant `id`, if the
    /// grant is still in force and the run has not used its share. Locks
    /// the grant, so calls made side by side cannot both take the last use.
    pub(crate) async fn use_grant(
        &self,
        user: &UserKey,
        id: Uuid,
        run: &str,
        call_id: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let per_run: Option<i32> = sqlx::query_scalar(
            "SELECT g.per_run FROM grants g
             JOIN crons c ON c.user_id = g.user_id AND c.id = g.routine_id
             WHERE g.id = $1 AND g.user_id = $2 AND g.revoked_at IS NULL AND g.expires_at > now()
             FOR UPDATE OF g",
        )
        .bind(id)
        .bind(user.as_str())
        .fetch_optional(&mut *tx)
        .await?;
        // An early return rolls back now: a dropped transaction would keep
        // its connection idle in it, holding the lock.
        let Some(per_run) = per_run else {
            tx.rollback().await?;
            return Ok(false);
        };
        let used: i64 =
            sqlx::query_scalar("SELECT count(*) FROM grant_uses WHERE grant_id=$1 AND run_id=$2")
                .bind(id)
                .bind(run)
                .fetch_one(&mut *tx)
                .await?;
        if used >= i64::from(per_run) {
            tx.rollback().await?;
            return Ok(false);
        }
        let inserted = sqlx::query(
            "INSERT INTO grant_uses (grant_id, run_id, call_id) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(run)
        .bind(call_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(inserted > 0)
    }

    /// Ends grant `id` of `user`; false when they have no such grant or
    /// already revoked it.
    pub(crate) async fn revoke_grant(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let revoked = sqlx::query(
            "UPDATE grants SET revoked_at = now()
             WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(id)
        .bind(user.as_str())
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(revoked > 0)
    }

    /// Makes grant `id` expire now, for tests of an expired grant.
    #[cfg(test)]
    pub(crate) async fn expire_grant(&self, id: Uuid) -> Result<()> {
        sqlx::query("UPDATE grants SET expires_at = now() - interval '1 second' WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
