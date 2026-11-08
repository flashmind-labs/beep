//! A project's branches and reviews of one before a merge, for the
//! window: listing branches, starting and reading a review, dismissing a
//! finding, and handing the rest to a worker or the project agent.

use crate::{
    beep::{Beep, Service, epoch_ms},
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        files::{self as direct, FALLBACK_DEFAULT},
    },
    projects::{
        branches::{Placed, placed},
        known, needs_update,
    },
};
use beep_proto::{
    BranchReview, CheckBranchReview, DismissReviewFinding, FixReviewFindings, GetBranchReview,
    ListBranchReviews, ListProjectBranches, MergeReviewedBranch, PostBranchReview, ProjectBranch,
    ProjectBranches, ProjectTask, ReviewFinding, ReviewHead, ReviewMerge, ReviewPr, ReviewShard,
    ReviewShardState, ReviewVerdict, StartBranchReview,
};
use serde::Serialize;
use std::time::Duration;
use uuid::Uuid;

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
            checks: Into::<&'static str>::into(pr.checks),
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
            state: Into::<&'static str>::into(&s.state),
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
            severity: Into::<&'static str>::into(f.severity),
            title: f.title,
            file: f.file,
            line: f.line,
            why: f.why,
            verdict: Into::<&'static str>::into(&f.verdict),
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
        let merge = Into::<&'static str>::into(&r.merge);
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
            state: Into::<&'static str>::into(t.state),
        }
    }
}

fn outdated(e: DesktopError) -> DesktopError {
    needs_update(e, "before it reviews branches")
}

/// `project`'s branches, read at a checkout on a machine of the mesh when
/// it has one; `None` from a server too old to list them.
pub(crate) async fn read_branches(
    beep: &Beep,
    fleet: &Fleet,
    project: String,
) -> Result<Option<ProjectBranches>> {
    if let Placed::Mesh(places) = placed(beep, fleet, &project).await?
        && let Some(place) = places.first()
    {
        let (default_branch, branches) = direct::branches(fleet, place, FALLBACK_DEFAULT).await?;
        return Ok(Some(ProjectBranches {
            machine_id: place.machine.to_string(),
            machine_name: fleet.name_of(place.machine),
            default_branch,
            branches,
        }));
    }
    known(beep.call(ListProjectBranches { project }).await)
}

/// `project`'s branches; `None` from a server too old to list them.
#[tauri::command]
pub async fn project_branches(
    beep: Service<'_>,
    fleet: Machines<'_>,
    project: String,
) -> Result<Option<BranchesView>> {
    Ok(read_branches(&beep, &fleet, project).await?.map(Into::into))
}

/// `project`'s newest review of each branch; `None` from a server too old
/// to review.
#[tauri::command]
pub async fn reviews_list(beep: Service<'_>, project: String) -> Result<Option<Vec<ReviewView>>> {
    Ok(known(beep.call(ListBranchReviews { project }).await)?
        .map(|rows| rows.into_iter().map(Into::into).collect()))
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
    known(
        beep.call_within(CheckBranchReview { id }, CHECK_TIMEOUT)
            .await,
    )
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
#[path = "../tests/projects_reviews.rs"]
mod tests;
