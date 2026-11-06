//! Branch reviews: which sessions read which diff shard, and what they found.

use super::Store;
use crate::{
    account::user::UserKey,
    protocol::{ReviewChecks, ReviewSeverity},
};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgRow};
use strum::{EnumString, IntoStaticStr};
use uuid::Uuid;

/// One review as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewRow {
    pub(crate) id: Uuid,
    pub(crate) project: String,
    pub(crate) branch: String,
    pub(crate) base: String,
    /// The node's key.
    pub(crate) endpoint: String,
    /// The checkout the reviewers read, on that node.
    pub(crate) folder: String,
    pub(crate) head: String,
    pub(crate) merge_base: String,
    pub(crate) head_at: DateTime<Utc>,
    pub(crate) changed_lines: u32,
    pub(crate) files: u32,
    /// `None` when git could not say.
    pub(crate) conflicts: Option<Vec<String>>,
    pub(crate) pr: Option<PrRow>,
    pub(crate) started_at: DateTime<Utc>,
    /// The detached worktree its reviewers read, until it is removed.
    pub(crate) worktree: Option<String>,
}

/// The reviewed branch's pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrRow {
    pub(crate) number: u64,
    pub(crate) url: String,
    pub(crate) checks: ReviewChecks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ShardState {
    Reading,
    Verifying,
    Done,
    Failed,
}

/// One shard of a review's diff and the sessions reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShardRow {
    pub(crate) shard: u32,
    pub(crate) area: String,
    pub(crate) paths: Vec<String>,
    pub(crate) files: u32,
    pub(crate) lines: u32,
    pub(crate) agent: String,
    /// The reviewer's session on the node.
    pub(crate) session: String,
    pub(crate) verifier: Option<String>,
    pub(crate) state: ShardState,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum Verdict {
    Verified,
    Dropped,
    /// The verifier could not say.
    Unchecked,
}

/// One finding a reviewer reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewFindingRow {
    pub(crate) id: u32,
    pub(crate) shard: u32,
    pub(crate) severity: ReviewSeverity,
    pub(crate) title: String,
    pub(crate) file: String,
    pub(crate) line: Option<u32>,
    pub(crate) why: String,
    /// `None` while the verifier works.
    pub(crate) verdict: Option<Verdict>,
    pub(crate) verdict_why: Option<String>,
    pub(crate) dismissed: bool,
}

const REVIEW_COLUMNS: &str = "id, project, branch, base, endpoint_id, folder, head, merge_base, \
     head_at, changed_lines, files, conflicts, pr_number, pr_url, pr_checks, started_at, worktree";

const SHARD_COLUMNS: &str =
    "shard, area, paths, files, lines, agent, session, verifier, state, error";

const FINDING_COLUMNS: &str =
    "id, shard, severity, title, file, line, why, verdict, verdict_why, dismissed";

fn checks_of(text: &str) -> ReviewChecks {
    match text {
        "pending" => ReviewChecks::Pending {},
        "passing" => ReviewChecks::Passing {},
        "failing" => ReviewChecks::Failing {},
        _ => ReviewChecks::None {},
    }
}

fn severity_of(text: &str) -> Result<ReviewSeverity> {
    Ok(match text {
        "high" => ReviewSeverity::High {},
        "medium" => ReviewSeverity::Medium {},
        "low" => ReviewSeverity::Low {},
        other => bail!("unknown severity {other}"),
    })
}

fn count(row: &PgRow, at: usize) -> Result<u32> {
    let value: i32 = row.try_get(at)?;
    Ok(u32::try_from(value)?)
}

fn to_int(value: u32) -> Result<i32> {
    Ok(i32::try_from(value)?)
}

fn review_from(row: &PgRow) -> Result<ReviewRow> {
    let number: Option<i64> = row.try_get(12)?;
    let url: Option<String> = row.try_get(13)?;
    let checks: Option<String> = row.try_get(14)?;
    let pr = match (number, url) {
        (Some(number), Some(url)) => Some(PrRow {
            number: u64::try_from(number)?,
            url,
            checks: checks_of(checks.as_deref().unwrap_or_default()),
        }),
        _ => None,
    };
    Ok(ReviewRow {
        id: row.try_get(0)?,
        project: row.try_get(1)?,
        branch: row.try_get(2)?,
        base: row.try_get(3)?,
        endpoint: row.try_get(4)?,
        folder: row.try_get(5)?,
        head: row.try_get(6)?,
        merge_base: row.try_get(7)?,
        head_at: row.try_get(8)?,
        changed_lines: count(row, 9)?,
        files: count(row, 10)?,
        conflicts: row.try_get(11)?,
        pr,
        started_at: row.try_get(15)?,
        worktree: row.try_get(16)?,
    })
}

fn shard_from(row: &PgRow) -> Result<ShardRow> {
    let state: String = row.try_get(8)?;
    Ok(ShardRow {
        shard: count(row, 0)?,
        area: row.try_get(1)?,
        paths: row.try_get(2)?,
        files: count(row, 3)?,
        lines: count(row, 4)?,
        agent: row.try_get(5)?,
        session: row.try_get(6)?,
        verifier: row.try_get(7)?,
        state: state.parse()?,
        error: row.try_get(9)?,
    })
}

fn finding_from(row: &PgRow) -> Result<ReviewFindingRow> {
    let severity: String = row.try_get(2)?;
    let line: Option<i32> = row.try_get(5)?;
    let verdict: Option<String> = row.try_get(7)?;
    Ok(ReviewFindingRow {
        id: count(row, 0)?,
        shard: count(row, 1)?,
        severity: severity_of(&severity)?,
        title: row.try_get(3)?,
        file: row.try_get(4)?,
        line: line.map(u32::try_from).transpose()?,
        why: row.try_get(6)?,
        verdict: verdict.map(|text| text.parse()).transpose()?,
        verdict_why: row.try_get(8)?,
        dismissed: row.try_get(9)?,
    })
}

impl Store {
    pub(crate) async fn review(&self, user: &UserKey, id: Uuid) -> Result<Option<ReviewRow>> {
        let row = sqlx::query(&format!(
            "SELECT {REVIEW_COLUMNS} FROM branch_reviews WHERE user_id=$1 AND id=$2"
        ))
        .bind(user.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(review_from).transpose()
    }

    /// `project`'s newest review per branch, newest first, at most `limit`.
    pub(crate) async fn reviews(
        &self,
        user: &UserKey,
        project: &str,
        limit: u32,
    ) -> Result<Vec<ReviewRow>> {
        let rows = sqlx::query(&format!(
            "SELECT * FROM (
                 SELECT DISTINCT ON (branch) {REVIEW_COLUMNS} FROM branch_reviews
                 WHERE user_id=$1 AND project=$2 ORDER BY branch, started_at DESC
             ) newest ORDER BY started_at DESC LIMIT $3"
        ))
        .bind(user.as_str())
        .bind(project)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(review_from).collect()
    }

    /// A review's shards, in order. The caller checks the review is theirs.
    pub(crate) async fn review_shards(&self, review: Uuid) -> Result<Vec<ShardRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {SHARD_COLUMNS} FROM branch_review_shards WHERE review_id=$1 ORDER BY shard"
        ))
        .bind(review)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(shard_from).collect()
    }

    /// A review's findings, by id. The caller checks the review is theirs.
    pub(crate) async fn review_findings(&self, review: Uuid) -> Result<Vec<ReviewFindingRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {FINDING_COLUMNS} FROM branch_review_findings WHERE review_id=$1 ORDER BY id"
        ))
        .bind(review)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(finding_from).collect()
    }

    /// Dismisses or restores one finding of the user's review; false when missing.
    pub(crate) async fn dismiss_finding(
        &self,
        user: &UserKey,
        review: Uuid,
        finding: u32,
        dismissed: bool,
    ) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE branch_review_findings f SET dismissed=$4
             FROM branch_reviews r
             WHERE f.review_id=r.id AND r.user_id=$1 AND r.id=$2 AND f.id=$3",
        )
        .bind(user.as_str())
        .bind(review)
        .bind(to_int(finding)?)
        .bind(dismissed)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }
}
