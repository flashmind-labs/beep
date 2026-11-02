//! Branch reviews: which reviewer and verifier sessions read which shard of
//! a branch's diff, and the findings they reported.

use super::Store;
use crate::{
    protocol::{ReviewChecks, ReviewSeverity},
    user::UserKey,
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

/// What the verifier said of one finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Judged {
    pub(crate) id: u32,
    pub(crate) verdict: Verdict,
    pub(crate) why: Option<String>,
}

const REVIEW_COLUMNS: &str = "id, project, branch, base, endpoint_id, folder, head, merge_base, \
     head_at, changed_lines, files, conflicts, pr_number, pr_url, pr_checks, started_at";

const SHARD_COLUMNS: &str =
    "shard, area, paths, files, lines, agent, session, verifier, state, error";

const FINDING_COLUMNS: &str =
    "id, shard, severity, title, file, line, why, verdict, verdict_why, dismissed";

fn checks_text(checks: ReviewChecks) -> &'static str {
    checks.into()
}

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
    /// Keeps a new review with its shards.
    pub(crate) async fn add_review(
        &self,
        user: &UserKey,
        review: &ReviewRow,
        shards: &[ShardRow],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&format!(
            "INSERT INTO branch_reviews (user_id, {REVIEW_COLUMNS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)"
        ))
        .bind(user.as_str())
        .bind(review.id)
        .bind(&review.project)
        .bind(&review.branch)
        .bind(&review.base)
        .bind(&review.endpoint)
        .bind(&review.folder)
        .bind(&review.head)
        .bind(&review.merge_base)
        .bind(review.head_at)
        .bind(to_int(review.changed_lines)?)
        .bind(to_int(review.files)?)
        .bind(&review.conflicts)
        .bind(
            review
                .pr
                .as_ref()
                .map(|pr| i64::try_from(pr.number))
                .transpose()?,
        )
        .bind(review.pr.as_ref().map(|pr| pr.url.clone()))
        .bind(review.pr.as_ref().map(|pr| checks_text(pr.checks)))
        .bind(review.started_at)
        .execute(&mut *tx)
        .await?;
        for shard in shards {
            let state: &'static str = shard.state.into();
            sqlx::query(&format!(
                "INSERT INTO branch_review_shards (review_id, {SHARD_COLUMNS})
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
            ))
            .bind(review.id)
            .bind(to_int(shard.shard)?)
            .bind(&shard.area)
            .bind(&shard.paths)
            .bind(to_int(shard.files)?)
            .bind(to_int(shard.lines)?)
            .bind(&shard.agent)
            .bind(&shard.session)
            .bind(&shard.verifier)
            .bind(state)
            .bind(&shard.error)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

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

    /// `project`'s newest review of each branch, newest first, at most
    /// `limit`.
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

    /// Keeps what a shard's reviewer reported and moves the shard from
    /// reading to `next`; false, changing nothing, when it no longer reads
    /// (another call got there first).
    pub(crate) async fn report_shard(
        &self,
        review: Uuid,
        shard: u32,
        findings: &[ReviewFindingRow],
        next: ShardState,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let next: &'static str = next.into();
        let moved = sqlx::query(
            "UPDATE branch_review_shards SET state=$3
             WHERE review_id=$1 AND shard=$2 AND state='reading'",
        )
        .bind(review)
        .bind(to_int(shard)?)
        .bind(next)
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() == 0 {
            return Ok(false);
        }
        for finding in findings {
            let severity: &'static str = finding.severity.into();
            let verdict: Option<&'static str> = finding.verdict.map(Into::into);
            sqlx::query(&format!(
                "INSERT INTO branch_review_findings (review_id, {FINDING_COLUMNS})
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
            ))
            .bind(review)
            .bind(to_int(finding.id)?)
            .bind(to_int(finding.shard)?)
            .bind(severity)
            .bind(&finding.title)
            .bind(&finding.file)
            .bind(finding.line.map(to_int).transpose()?)
            .bind(&finding.why)
            .bind(verdict)
            .bind(&finding.verdict_why)
            .bind(finding.dismissed)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub(crate) async fn set_review_verifier(
        &self,
        review: Uuid,
        shard: u32,
        session: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE branch_review_shards SET verifier=$3 WHERE review_id=$1 AND shard=$2")
            .bind(review)
            .bind(to_int(shard)?)
            .bind(session)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Keeps the verifier's verdicts and marks the shard done; findings it
    /// gave none for are unchecked. False, changing nothing, when the shard
    /// no longer waits for its verifier.
    pub(crate) async fn judge_shard(
        &self,
        review: Uuid,
        shard: u32,
        judged: &[Judged],
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let moved = sqlx::query(
            "UPDATE branch_review_shards SET state='done'
             WHERE review_id=$1 AND shard=$2 AND state='verifying'",
        )
        .bind(review)
        .bind(to_int(shard)?)
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() == 0 {
            return Ok(false);
        }
        for one in judged {
            let verdict: &'static str = one.verdict.into();
            sqlx::query(
                "UPDATE branch_review_findings SET verdict=$4, verdict_why=$5
                 WHERE review_id=$1 AND shard=$2 AND id=$3",
            )
            .bind(review)
            .bind(to_int(shard)?)
            .bind(to_int(one.id)?)
            .bind(verdict)
            .bind(&one.why)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE branch_review_findings SET verdict='unchecked'
             WHERE review_id=$1 AND shard=$2 AND verdict IS NULL",
        )
        .bind(review)
        .bind(to_int(shard)?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Marks a shard failed, saying why, while it is still `from`.
    pub(crate) async fn fail_shard(
        &self,
        review: Uuid,
        shard: u32,
        from: ShardState,
        why: &str,
    ) -> Result<bool> {
        let from: &'static str = from.into();
        let done = sqlx::query(
            "UPDATE branch_review_shards SET state='failed', error=$4
             WHERE review_id=$1 AND shard=$2 AND state=$3",
        )
        .bind(review)
        .bind(to_int(shard)?)
        .bind(from)
        .bind(why)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Dismisses one finding of the user's review, or brings it back;
    /// false when they have no such finding.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_db::TestDb;
    use chrono::SubsecRound;

    fn review(project: &str, branch: &str, at: DateTime<Utc>) -> ReviewRow {
        // Postgres keeps microseconds.
        let at = at.trunc_subsecs(6);
        ReviewRow {
            id: Uuid::new_v4(),
            project: project.into(),
            branch: branch.into(),
            base: "main".into(),
            endpoint: "node".into(),
            folder: "/srv/project".into(),
            head: "3f2c1a0".into(),
            merge_base: "1a2b3c4".into(),
            head_at: at,
            changed_lines: 412,
            files: 9,
            conflicts: Some(vec!["src/lib.rs".into()]),
            pr: Some(PrRow {
                number: 41,
                url: "https://example.com/acme/widget/pull/41".into(),
                checks: ReviewChecks::Passing {},
            }),
            started_at: at,
        }
    }

    fn shard(n: u32) -> ShardRow {
        ShardRow {
            shard: n,
            area: "apps/web".into(),
            paths: vec!["apps/web/src/login.ts".into()],
            files: 1,
            lines: 20,
            agent: "claude".into(),
            session: format!("s{n}"),
            verifier: None,
            state: ShardState::Reading,
            error: None,
        }
    }

    fn finding(id: u32, shard: u32) -> ReviewFindingRow {
        ReviewFindingRow {
            id,
            shard,
            severity: ReviewSeverity::High {},
            title: "The reset screen says which field is wrong".into(),
            file: "apps/web/src/reset.ts".into(),
            line: Some(18),
            why: "It tells someone the email exists".into(),
            verdict: None,
            verdict_why: None,
            dismissed: false,
        }
    }

    #[tokio::test]
    async fn a_review_moves_from_reading_to_verified_once() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let bob = UserKey::for_test("bob");
        let one = review("acme/widget", "login-copy", Utc::now());
        store
            .add_review(&user, &one, &[shard(0), shard(1)])
            .await
            .unwrap();
        assert_eq!(
            store.review(&user, one.id).await.unwrap(),
            Some(one.clone())
        );
        assert!(store.review(&bob, one.id).await.unwrap().is_none());

        let found = [finding(0, 0), finding(1, 0)];
        assert!(
            store
                .report_shard(one.id, 0, &found, ShardState::Verifying)
                .await
                .unwrap()
        );
        // A second report of the same shard changes nothing.
        assert!(
            !store
                .report_shard(one.id, 0, &found, ShardState::Verifying)
                .await
                .unwrap()
        );
        store.set_review_verifier(one.id, 0, "v0").await.unwrap();
        let judged = [Judged {
            id: 1,
            verdict: Verdict::Dropped,
            why: Some("A macro uses it".into()),
        }];
        assert!(store.judge_shard(one.id, 0, &judged).await.unwrap());
        assert!(!store.judge_shard(one.id, 0, &judged).await.unwrap());
        let findings = store.review_findings(one.id).await.unwrap();
        assert_eq!(findings[0].verdict, Some(Verdict::Unchecked));
        assert_eq!(findings[1].verdict, Some(Verdict::Dropped));
        assert_eq!(findings[1].verdict_why.as_deref(), Some("A macro uses it"));

        assert!(
            store
                .fail_shard(one.id, 1, ShardState::Reading, "no report")
                .await
                .unwrap()
        );
        let shards = store.review_shards(one.id).await.unwrap();
        assert_eq!(shards[0].state, ShardState::Done);
        assert_eq!(shards[0].verifier.as_deref(), Some("v0"));
        assert_eq!(shards[1].state, ShardState::Failed);
        assert_eq!(shards[1].error.as_deref(), Some("no report"));

        assert!(!store.dismiss_finding(&bob, one.id, 0, true).await.unwrap());
        assert!(store.dismiss_finding(&user, one.id, 0, true).await.unwrap());
        assert!(store.review_findings(one.id).await.unwrap()[0].dismissed);
    }

    #[tokio::test]
    async fn the_newest_review_of_each_branch_is_listed() {
        let db = TestDb::new().await.unwrap();
        let store = db.store().await.unwrap();
        let user = UserKey::for_test("alice");
        let now = Utc::now();
        let old = review(
            "acme/widget",
            "login-copy",
            now - chrono::Duration::hours(2),
        );
        let new = review(
            "acme/widget",
            "login-copy",
            now - chrono::Duration::hours(1),
        );
        let other = review("acme/widget", "spike", now);
        let elsewhere = review("acme/relay", "spike", now);
        for one in [&old, &new, &other, &elsewhere] {
            store.add_review(&user, one, &[]).await.unwrap();
        }
        let listed: Vec<Uuid> = store
            .reviews(&user, "acme/widget", 20)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(listed, vec![other.id, new.id]);
    }
}
