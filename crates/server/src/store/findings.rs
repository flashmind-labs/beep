//! Findings from the improvement watcher, one row per problem and user; a user only sees findings from their own chats.

use super::Store;
use crate::memory::watchers::{Cause, FindingKind, FindingStatus};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// A problem as one review saw it.
pub struct NewFinding<'a> {
    pub fingerprint: &'a str,
    pub kind: FindingKind,
    pub summary: &'a str,
    pub steps: &'a str,
    pub cause: Cause,
    pub cause_detail: &'a str,
    pub fix: &'a str,
}

/// Where a review saw it.
pub struct Sighting<'a> {
    pub user: &'a str,
    pub chat: Uuid,
    pub first_seq: i64,
    pub last_seq: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FindingRow {
    pub id: Uuid,
    pub fingerprint: String,
    pub kind: FindingKind,
    pub summary: String,
    pub steps: String,
    pub cause: Cause,
    pub cause_detail: String,
    pub fix: String,
    pub status: FindingStatus,
    /// How many times it was seen.
    pub seen: i32,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    /// Its latest sighting in a chat of the reading user; always set when read back here.
    pub sighting: Option<SeenIn>,
}

/// A chat a finding was seen in, between two messages.
#[derive(Debug, Clone, PartialEq)]
pub struct SeenIn {
    pub chat: Uuid,
    pub first_seq: i64,
    pub last_seq: i64,
}

/// The columns `finding` reads: `findings f` and the reader's latest sighting `s`.
const COLUMNS: &str = "f.id, f.fingerprint, f.kind, f.summary, f.steps, f.cause, f.cause_detail,
    f.fix, f.status, f.seen, f.first_seen, f.last_seen, s.chat_id, s.first_seq, s.last_seq";

/// `findings f` joined to its latest sighting in a chat of user `$1`; unseen findings are left out.
const FROM: &str = "findings f JOIN LATERAL (
        SELECT chat_id, first_seq, last_seq FROM finding_sightings
        WHERE finding_id = f.id AND user_id = $1 ORDER BY seen_at DESC LIMIT 1
    ) s ON TRUE";

/// The per-user fingerprint a finding is stored under.
fn keyed(user: &str, fingerprint: &str) -> String {
    format!("{user}|{fingerprint}")
}

/// `stored` as the watcher made it; findings from before per-user keys have none.
fn unkeyed(reader: &str, stored: String) -> String {
    match stored.strip_prefix(&keyed(reader, "")) {
        Some(fingerprint) => fingerprint.to_owned(),
        None => stored,
    }
}

fn finding(row: &PgRow, reader: &str) -> Result<FindingRow> {
    let text = |i: usize| -> Result<String> { Ok(row.try_get(i)?) };
    let chat: Option<Uuid> = row.try_get(12)?;
    Ok(FindingRow {
        id: row.try_get(0)?,
        fingerprint: unkeyed(reader, text(1)?),
        kind: text(2)?.parse().unwrap_or(FindingKind::Other),
        summary: text(3)?,
        steps: text(4)?,
        cause: text(5)?.parse().unwrap_or(Cause::Other),
        cause_detail: text(6)?,
        fix: text(7)?,
        status: text(8)?.parse().context("unknown finding status")?,
        seen: row.try_get(9)?,
        first_seen: row.try_get(10)?,
        last_seen: row.try_get(11)?,
        sighting: match chat {
            Some(chat) => Some(SeenIn {
                chat,
                first_seq: row.try_get(13)?,
                last_seq: row.try_get(14)?,
            }),
            None => None,
        },
    })
}

impl Store {
    /// Records `finding` seen at `seen`, or bumps an existing one (reopening if resolved, not if dismissed); returns its id.
    pub(crate) async fn record_finding(
        &self,
        finding: &NewFinding<'_>,
        seen: &Sighting<'_>,
        at: DateTime<Utc>,
    ) -> Result<Uuid> {
        let kind: &str = finding.kind.into();
        let cause: &str = finding.cause.into();
        let mut tx = self.pool.begin().await?;
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO findings (id, fingerprint, kind, summary, steps, cause, cause_detail, fix,
                first_seen, last_seen, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9, $9)
             ON CONFLICT (fingerprint) DO UPDATE SET
                seen = findings.seen + 1,
                last_seen = GREATEST(findings.last_seen, EXCLUDED.last_seen),
                status = CASE WHEN findings.status = 'resolved' THEN 'open' ELSE findings.status END,
                updated_at = EXCLUDED.updated_at
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(keyed(seen.user, finding.fingerprint))
        .bind(kind)
        .bind(finding.summary)
        .bind(finding.steps)
        .bind(cause)
        .bind(finding.cause_detail)
        .bind(finding.fix)
        .bind(at)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO finding_sightings (finding_id, user_id, chat_id, first_seq, last_seq, seen_at)
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(seen.user)
        .bind(seen.chat)
        .bind(seen.first_seq)
        .bind(seen.last_seq)
        .bind(at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Findings seen in `reader`'s chats, newest sighting first, filtered by `status` when given.
    pub(crate) async fn findings(
        &self,
        reader: &str,
        status: Option<FindingStatus>,
        limit: i64,
    ) -> Result<Vec<FindingRow>> {
        let status: Option<&str> = status.map(Into::into);
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM {FROM}
             WHERE ($2::text IS NULL OR f.status = $2::text)
             ORDER BY f.last_seen DESC LIMIT $3"
        ))
        .bind(reader)
        .bind(status)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| finding(row, reader)).collect()
    }

    /// Sets the status of finding `id`, `None` when `reader` has none such.
    pub(crate) async fn set_finding_status(
        &self,
        reader: &str,
        id: Uuid,
        status: FindingStatus,
        at: DateTime<Utc>,
    ) -> Result<Option<FindingRow>> {
        let status: &str = status.into();
        let changed = sqlx::query(
            "UPDATE findings SET status = $2, updated_at = $3 WHERE id = $1 AND EXISTS (
                SELECT 1 FROM finding_sightings WHERE finding_id = $1 AND user_id = $4
             )",
        )
        .bind(id)
        .bind(status)
        .bind(at)
        .bind(reader)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if changed == 0 {
            return Ok(None);
        }

        let row = sqlx::query(&format!("SELECT {COLUMNS} FROM {FROM} WHERE f.id = $2"))
            .bind(reader)
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        finding(&row, reader).map(Some)
    }
}

#[cfg(test)]
#[path = "../tests/store_findings.rs"]
mod tests;
