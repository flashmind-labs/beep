//! Branches with work before an import, and giving a branch a worktree.

use crate::{
    beep::{Service, epoch_ms},
    error::Result,
    fleet::projects::{FleetState, desk},
    projects::needs_update,
    projects::reviews::BranchRow,
};
use beep_proto::{AddBranchWorktree, ImportCheckout, ImportFork, ImportPreview, WorkBranch};
use serde::Serialize;
use std::time::Duration;

/// Adding a worktree may fetch the branch first.
const ADD_TIMEOUT: Duration = Duration::from_secs(150);

#[derive(Debug, Serialize)]
pub struct CheckoutRow {
    machine_id: String,
    machine_name: String,
    folder: String,
    remote: String,
}

impl From<ImportCheckout> for CheckoutRow {
    fn from(c: ImportCheckout) -> Self {
        Self {
            machine_id: c.machine_id,
            machine_name: c.machine_name,
            folder: c.folder,
            remote: c.remote,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WorkBranchRow {
    name: String,
    pr: Option<u64>,
    pr_title: String,
    ahead: u32,
    worktree: Option<String>,
    in_checkout: bool,
    /// In milliseconds since the Unix epoch.
    committed_at: Option<u64>,
}

impl From<WorkBranch> for WorkBranchRow {
    fn from(b: WorkBranch) -> Self {
        Self {
            name: b.name,
            pr: b.pr,
            pr_title: b.pr_title,
            ahead: b.ahead,
            worktree: b.worktree,
            in_checkout: b.in_checkout,
            committed_at: b.committed_at.map(epoch_ms),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PreviewView {
    project: String,
    checkouts: Vec<CheckoutRow>,
    default_branch: String,
    branches: Vec<WorkBranchRow>,
    note: String,
}

impl From<ImportPreview> for PreviewView {
    fn from(p: ImportPreview) -> Self {
        Self {
            project: p.project,
            checkouts: p.checkouts.into_iter().map(Into::into).collect(),
            default_branch: p.default_branch,
            branches: p.branches.into_iter().map(Into::into).collect(),
            note: p.note,
        }
    }
}

/// What importing `repo` would find.
#[tauri::command]
pub async fn preview_import(fleet: FleetState<'_>, repo: String) -> Result<Option<PreviewView>> {
    Ok(Some(desk(&fleet).await?.preview(&repo).await?.into()))
}

/// The user's fork that importing `repo` would clone.
#[tauri::command]
pub async fn find_import_fork(fleet: FleetState<'_>, repo: String) -> Result<Option<ImportFork>> {
    Ok(Some(desk(&fleet).await?.find_fork(&repo).await?))
}

/// Gives `branch` of `project` a worktree on `machine_id`.
#[tauri::command]
pub async fn add_branch_worktree(
    beep: Service<'_>,
    project: String,
    branch: String,
    machine_id: String,
) -> Result<BranchRow> {
    let request = AddBranchWorktree {
        project,
        branch,
        machine_id,
    };
    let branch = beep.call_within(request, ADD_TIMEOUT).await;
    Ok(branch
        .map_err(|e| needs_update(e, "before it adds worktrees"))?
        .into())
}
