//! Branches with work before an import, and giving a branch a worktree.

use crate::{
    beep::{Beep, Service, epoch_ms},
    error::Result,
    fleet::{
        Fleet, Machines,
        files::{self as direct, Place},
    },
    projects::reviews::BranchRow,
    projects::{known, needs_update},
};
use beep_proto::{
    AddBranchWorktree, FindImportFork, ImportCheckout, ImportFork, ImportPreview,
    ListProjectMachines, PreviewImport, WorkBranch,
};
use serde::Serialize;
use std::time::Duration;

/// What a project's default branch is taken to be until its checkout says.
const FALLBACK_DEFAULT: &str = "main";

/// Where `project` has a checkout on a machine of the mesh, as the server
/// records it; none without a server or a record.
pub(crate) async fn places(beep: &Beep, fleet: &Fleet, project: &str) -> Vec<Place> {
    let machines = beep
        .call(ListProjectMachines {
            project: project.to_owned(),
        })
        .await;
    crate::fleet::extra(machines)
        .into_iter()
        .filter(|machine| machine.online)
        .filter_map(|machine| {
            Some(Place {
                machine: fleet.member(&machine.machine_id)?,
                folder: machine.folder?,
            })
        })
        .collect()
}

/// Adding a worktree may fetch the branch first.
const ADD_TIMEOUT: Duration = Duration::from_secs(150);
/// Previewing asks every online machine.
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(90);

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

/// What importing `repo` would find; `None` from a server too old to say.
#[tauri::command]
pub async fn preview_import(beep: Service<'_>, repo: String) -> Result<Option<PreviewView>> {
    Ok(known(
        beep.call_within(PreviewImport { repo }, PREVIEW_TIMEOUT)
            .await,
    )?
    .map(Into::into))
}

/// The user's fork that importing `repo` would clone; `None` from a server
/// too old to say.
#[tauri::command]
pub async fn find_import_fork(beep: Service<'_>, repo: String) -> Result<Option<ImportFork>> {
    known(
        beep.call_within(FindImportFork { repo }, PREVIEW_TIMEOUT)
            .await,
    )
}

/// Gives `branch` of `project` a worktree on `machine_id`.
#[tauri::command]
pub async fn add_branch_worktree(
    beep: Service<'_>,
    fleet: Machines<'_>,
    project: String,
    branch: String,
    machine_id: String,
) -> Result<BranchRow> {
    if let Some(machine) = fleet.member(&machine_id) {
        let place = places(&beep, &fleet, &project)
            .await
            .into_iter()
            .find(|place| place.machine == machine);
        if let Some(place) = place {
            let added = direct::add_worktree(&fleet, &place, &branch, FALLBACK_DEFAULT).await?;
            return Ok(added.into());
        }
    }
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
