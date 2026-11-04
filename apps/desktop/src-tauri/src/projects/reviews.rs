//! A project's branches and reviews of one before a merge, for the
//! window: listing branches, starting and reading a review, dismissing a
//! finding, and handing the rest to a worker or the project agent.

use crate::{
    beep::{Beep, epoch_ms},
    error::{DesktopError, Result},
    projects::{needs_update, unknown_op},
};
use beep_proto::{
    BranchReview, CheckBranchReview, DismissReviewFinding, FixReviewFindings, GetBranchReview,
    ListBranchReviews, ListProjectBranches, MergeReviewedBranch, PostBranchReview, ProjectBranch,
    ProjectBranches, ProjectTask, ReviewFinding, ReviewHead, ReviewMerge, ReviewPr, ReviewShard,
    ReviewShardState, ReviewVerdict, StartBranchReview,
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri::State;
use uuid::Uuid;

type Service<'a> = State<'a, Arc<Beep>>;

/// Starting a review fetches and starts a session per shard first.
const START_TIMEOUT: Duration = Duration::from_secs(150);
/// Checking where a branch is now fetches it first.
const CHECK_TIMEOUT: Duration = Duration::from_secs(45);

/// One branch as the window shows it.
#[derive(Debug, Serialize)]
pub struct BranchRow {
    name: String,
    worktree: Option<String>,
    ahead: u32,
    behind: u32,
    head: String,
    subject: String,
    /// In milliseconds since the Unix epoch.
    committed_at: u64,
    local: bool,
    pushed: bool,
}

impl From<ProjectBranch> for BranchRow {
    fn from(b: ProjectBranch) -> Self {
        Self {
            name: b.name,
            worktree: b.worktree,
            ahead: b.ahead,
            behind: b.behind,
            head: b.head,
            subject: b.subject,
            committed_at: epoch_ms(b.committed_at),
            local: b.local,
            pushed: b.pushed,
        }
    }
}

/// A project's branches on the machine that read them.
#[derive(Debug, Serialize)]
pub struct BranchesView {
    machine_id: String,
    machine_name: String,
    default_branch: String,
    branches: Vec<BranchRow>,
}

impl From<ProjectBranches> for BranchesView {
    fn from(b: ProjectBranches) -> Self {
        Self {
            machine_id: b.machine_id,
            machine_name: b.machine_name,
            default_branch: b.default_branch,
            branches: b.branches.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PrView {
    number: u64,
    url: String,
    /// `none`, `pending`, `passing` or `failing`.
    checks: &'static str,
}

impl From<ReviewPr> for PrView {
    fn from(pr: ReviewPr) -> Self {
        Self {
            number: pr.number,
            url: pr.url,
            checks: pr.checks.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ShardView {
    area: String,
    files: u32,
    lines: u32,
    agent: String,
    /// `reading`, `verifying`, `done` or `failed`.
    state: &'static str,
    /// What the reviewer does now while reading; why it failed.
    note: String,
    found: u32,
}

impl From<ReviewShard> for ShardView {
    fn from(s: ReviewShard) -> Self {
        let note = match &s.state {
            ReviewShardState::Reading { doing } => doing.clone(),
            ReviewShardState::Failed { why } => why.clone(),
            _ => String::new(),
        };
        Self {
            state: (&s.state).into(),
            note,
            area: s.area,
            files: s.files,
            lines: s.lines,
            agent: s.agent,
            found: s.found,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FindingView {
    id: u32,
    area: String,
    /// `high`, `medium` or `low`.
    severity: &'static str,
    title: String,
    file: String,
    line: Option<u32>,
    why: String,
    /// `waiting`, `verified`, `dropped` or `unchecked`.
    verdict: &'static str,
    /// Why the verifier dropped it.
    dropped_why: String,
    dismissed: bool,
}

impl From<ReviewFinding> for FindingView {
    fn from(f: ReviewFinding) -> Self {
        let dropped_why = match &f.verdict {
            ReviewVerdict::Dropped { why } => why.clone(),
            _ => String::new(),
        };
        Self {
            id: f.id,
            area: f.area,
            severity: f.severity.into(),
            title: f.title,
            file: f.file,
            line: f.line,
            why: f.why,
            verdict: (&f.verdict).into(),
            dropped_why,
            dismissed: f.dismissed,
        }
    }
}

/// One review as the window shows it.
#[derive(Debug, Serialize)]
pub struct ReviewView {
    id: Uuid,
    project: String,
    branch: String,
    base: String,
    head: String,
    /// In milliseconds since the Unix epoch.
    head_at: u64,
    started_at: u64,
    machine_name: String,
    changed_lines: u32,
    files: u32,
    /// `clean`, `conflicts` or `unknown`.
    merge: &'static str,
    conflicts: Vec<String>,
    pr: Option<PrView>,
    shards: Vec<ShardView>,
    findings: Vec<FindingView>,
}

impl From<BranchReview> for ReviewView {
    fn from(r: BranchReview) -> Self {
        let merge: &'static str = (&r.merge).into();
        let conflicts = match r.merge {
            ReviewMerge::Conflicts { files } => files,
            _ => Vec::new(),
        };
        Self {
            id: r.id,
            project: r.project,
            branch: r.branch,
            base: r.base,
            head: r.head,
            head_at: epoch_ms(r.head_at),
            started_at: epoch_ms(r.started_at),
            machine_name: r.machine_name,
            changed_lines: r.changed_lines,
            files: r.files,
            merge,
            conflicts,
            pr: r.pr.map(Into::into),
            shards: r.shards.into_iter().map(Into::into).collect(),
            findings: r.findings.into_iter().map(Into::into).collect(),
        }
    }
}

/// The task a fix started.
#[derive(Debug, Serialize)]
pub struct FixView {
    id: Uuid,
    slug: String,
    title: String,
    /// `planned`, `running` and so on.
    state: &'static str,
}

impl From<ProjectTask> for FixView {
    fn from(t: ProjectTask) -> Self {
        Self {
            id: t.id,
            slug: t.slug,
            title: t.title,
            state: t.state.into(),
        }
    }
}

fn outdated(e: DesktopError) -> DesktopError {
    needs_update(e, "before it reviews branches")
}

/// `project`'s branches; `None` from a server too old to list them.
#[tauri::command]
pub async fn project_branches(beep: Service<'_>, project: String) -> Result<Option<BranchesView>> {
    match beep.call(ListProjectBranches { project }).await {
        Ok(branches) => Ok(Some(branches.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `project`'s newest review of each branch; `None` from a server too old
/// to review.
#[tauri::command]
pub async fn reviews_list(beep: Service<'_>, project: String) -> Result<Option<Vec<ReviewView>>> {
    match beep.call(ListBranchReviews { project }).await {
        Ok(rows) => Ok(Some(rows.into_iter().map(Into::into).collect())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn review_start(
    beep: Service<'_>,
    project: String,
    branch: String,
) -> Result<ReviewView> {
    let started = beep
        .call_within(StartBranchReview { project, branch }, START_TIMEOUT)
        .await
        .map_err(outdated)?;
    Ok(started.into())
}

#[tauri::command]
pub async fn review_get(beep: Service<'_>, id: Uuid) -> Result<ReviewView> {
    let review = beep.call(GetBranchReview { id }).await.map_err(outdated)?;
    Ok(review.into())
}

/// Where the review's branch is now; `None` from a server too old to say.
#[tauri::command]
pub async fn review_check(beep: Service<'_>, id: Uuid) -> Result<Option<ReviewHead>> {
    match beep
        .call_within(CheckBranchReview { id }, CHECK_TIMEOUT)
        .await
    {
        Ok(now) => Ok(Some(now)),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn review_dismiss(
    beep: Service<'_>,
    review: Uuid,
    finding: u32,
    dismissed: bool,
) -> Result<ReviewView> {
    let shown = beep
        .call(DismissReviewFinding {
            review,
            finding,
            dismissed,
        })
        .await
        .map_err(outdated)?;
    Ok(shown.into())
}

#[tauri::command]
pub async fn review_fix(beep: Service<'_>, review: Uuid) -> Result<FixView> {
    let task = beep
        .call(FixReviewFindings { review })
        .await
        .map_err(outdated)?;
    Ok(task.into())
}

/// Asks the project agent to post the review; gives the project chat.
#[tauri::command]
pub async fn review_post(beep: Service<'_>, review: Uuid) -> Result<Uuid> {
    beep.call(PostBranchReview { review })
        .await
        .map_err(outdated)
}

/// Asks the project agent to merge; gives the project chat.
#[tauri::command]
pub async fn review_merge(beep: Service<'_>, review: Uuid) -> Result<Uuid> {
    beep.call(MergeReviewedBranch { review })
        .await
        .map_err(outdated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::{ReviewChecks, ReviewSeverity};
    use std::time::UNIX_EPOCH;

    #[test]
    fn a_review_reads_plainly() {
        let view = ReviewView::from(BranchReview {
            id: Uuid::nil(),
            project: "acme/widget".into(),
            branch: "login-copy".into(),
            base: "main".into(),
            head: "3f2c1a0".into(),
            head_at: UNIX_EPOCH,
            started_at: UNIX_EPOCH,
            machine_name: "build-box".into(),
            changed_lines: 412,
            files: 9,
            merge: ReviewMerge::Conflicts {
                files: vec!["Cargo.toml".into()],
            },
            pr: Some(ReviewPr {
                number: 41,
                url: "https://example.com/pull/41".into(),
                checks: ReviewChecks::Failing {},
            }),
            shards: vec![ReviewShard {
                area: "apps/web".into(),
                files: 6,
                lines: 268,
                agent: "claude".into(),
                state: ReviewShardState::Reading {
                    doing: "Read Login.tsx".into(),
                },
                found: 0,
            }],
            findings: vec![ReviewFinding {
                id: 3,
                area: "apps/web".into(),
                severity: ReviewSeverity::Medium {},
                title: "Unused import".into(),
                file: "src/limit.rs".into(),
                line: None,
                why: "Dead".into(),
                verdict: ReviewVerdict::Dropped {
                    why: "A macro uses it".into(),
                },
                dismissed: false,
            }],
        });
        assert_eq!(view.merge, "conflicts");
        assert_eq!(view.conflicts, ["Cargo.toml"]);
        assert_eq!(view.pr.as_ref().map(|pr| pr.checks), Some("failing"));
        assert_eq!(view.shards[0].state, "reading");
        assert_eq!(view.shards[0].note, "Read Login.tsx");
        assert_eq!(view.findings[0].severity, "medium");
        assert_eq!(view.findings[0].verdict, "dropped");
        assert_eq!(view.findings[0].dropped_why, "A macro uses it");
    }
}
