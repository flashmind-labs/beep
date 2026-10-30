//! Findings: where Beep failed a user, as the improvement watcher proposed
//! them, one row per problem however many chats it came up in.

use super::Store;
use crate::watchers::{Cause, FindingKind, FindingStatus};
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

/// A stored finding.
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
    /// Its latest sighting in a chat of the user it was read for.
    pub sighting: Option<SeenIn>,
}

/// A chat a finding was seen in, between two messages.
#[derive(Debug, Clone, PartialEq)]
pub struct SeenIn {
    pub chat: Uuid,
    pub first_seq: i64,
    pub last_seq: i64,
}

/// The columns `finding` reads: `findings f` and the reader's latest
/// sighting `s`.
const COLUMNS: &str = "f.id, f.fingerprint, f.kind, f.summary, f.steps, f.cause, f.cause_detail,
    f.fix, f.status, f.seen, f.first_seen, f.last_seen, s.chat_id, s.first_seq, s.last_seq";

/// `findings f` with the latest sighting in a chat of the user `$1`.
const FROM: &str = "findings f LEFT JOIN LATERAL (
        SELECT chat_id, first_seq, last_seq FROM finding_sightings
        WHERE finding_id = f.id AND user_id = $1 ORDER BY seen_at DESC LIMIT 1
    ) s ON TRUE";

fn finding(row: &PgRow) -> Result<FindingRow> {
    let text = |i: usize| -> Result<String> { Ok(row.try_get(i)?) };
    let chat: Option<Uuid> = row.try_get(12)?;
    Ok(FindingRow {
        id: row.try_get(0)?,
        fingerprint: text(1)?,
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
    /// Records `finding` seen at `seen`. One with the same fingerprint
    /// counts once more instead, and opens again if it was resolved; a
    /// dismissed one stays dismissed. Returns its id.
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
        .bind(finding.fingerprint)
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

    /// Findings, most recently seen first, only those with `status` when
    /// given, each with its latest sighting in a chat of `reader`.
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
        rows.iter().map(finding).collect()
    }

    /// Sets the status of finding `id`, `None` when there is none.
    pub(crate) async fn set_finding_status(
        &self,
        reader: &str,
        id: Uuid,
        status: FindingStatus,
        at: DateTime<Utc>,
    ) -> Result<Option<FindingRow>> {
        let status: &str = status.into();
        let changed = sqlx::query("UPDATE findings SET status = $2, updated_at = $3 WHERE id = $1")
            .bind(id)
            .bind(status)
            .bind(at)
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
        finding(&row).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{TestDb, test_chat, test_turn},
        user::UserKey,
    };
    use chrono::TimeDelta;

    fn new(fingerprint: &str) -> NewFinding<'_> {
        NewFinding {
            fingerprint,
            kind: FindingKind::WrongTool,
            summary: "Searched mail for a calendar event.",
            steps: "Ask when the next meeting is.",
            cause: Cause::Prompt,
            cause_detail: "The prompt does not say calendars hold meetings.",
            fix: "Say so in the prompt.",
        }
    }

    #[tokio::test]
    async fn the_same_problem_in_other_chats_is_one_finding() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let (ours, theirs) = (test_chat("ours"), test_chat("theirs"));
        test_turn(&store, &UserKey::for_test("alice"), ours, "when", 1_000)
            .await
            .unwrap();
        test_turn(&store, &UserKey::for_test("bob"), theirs, "when", 2_000)
            .await
            .unwrap();
        let start = DateTime::UNIX_EPOCH + TimeDelta::days(1);
        let seen = |user, chat, first_seq| Sighting {
            user,
            chat,
            first_seq,
            last_seq: first_seq + 1,
        };
        let fingerprint = "wrong_tool:gmail:calendar-in-mail";
        let id = store
            .record_finding(&new(fingerprint), &seen("alice", ours, 1), start)
            .await
            .unwrap();
        let later = start + TimeDelta::hours(1);
        let again = store
            .record_finding(&new(fingerprint), &seen("bob", theirs, 7), later)
            .await
            .unwrap();
        assert_eq!(id, again);
        let other = store
            .record_finding(
                &new("loop:web_search:same-query"),
                &seen("bob", theirs, 9),
                start,
            )
            .await
            .unwrap();
        assert_ne!(id, other);

        let found = store.findings("alice", None, 10).await.unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, id, "most recently seen first");
        assert_eq!(found[0].seen, 2);
        assert_eq!((found[0].first_seen, found[0].last_seen), (start, later));
        // Alice reads where she saw it, never Bob's chat.
        assert_eq!(
            found[0].sighting,
            Some(SeenIn {
                chat: ours,
                first_seq: 1,
                last_seq: 2
            })
        );
        assert_eq!(found[1].sighting, None);

        let dismissed = store
            .set_finding_status("alice", other, FindingStatus::Dismissed, later)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(dismissed.status, FindingStatus::Dismissed);
        let open = store
            .findings("alice", Some(FindingStatus::Open), 10)
            .await
            .unwrap();
        assert_eq!(open.iter().map(|f| f.id).collect::<Vec<_>>(), [id]);
        assert!(
            store
                .set_finding_status("alice", Uuid::new_v4(), FindingStatus::Resolved, later)
                .await
                .unwrap()
                .is_none()
        );

        // A resolved problem that comes back opens again; a dismissed one does not.
        store
            .set_finding_status("alice", id, FindingStatus::Resolved, later)
            .await
            .unwrap();
        store
            .record_finding(&new(fingerprint), &seen("alice", ours, 20), later)
            .await
            .unwrap();
        store
            .record_finding(
                &new("loop:web_search:same-query"),
                &seen("bob", theirs, 30),
                later,
            )
            .await
            .unwrap();
        let all = store.findings("alice", None, 10).await.unwrap();
        let status = |id| all.iter().find(|f| f.id == id).map(|f| (f.status, f.seen));
        assert_eq!(status(id), Some((FindingStatus::Open, 3)));
        assert_eq!(status(other), Some((FindingStatus::Dismissed, 2)));
    }
}
