//! Branches with work before an import, and giving a branch a worktree.

use crate::{
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        files::{self as direct, FALLBACK_DEFAULT, Place},
        projects::{FleetState, desk},
    },
};
use beep_proto::{
    ImportCheckout, ImportFork, ImportPreview, ProjectBranch, ProjectBranches, WorkBranch,
};
use serde::Serialize;

/// Where `project` has a checkout on a machine of the mesh that is online, nearest first. With
/// none online, or a project the mesh does not record, that is an error.
pub(crate) fn placed(fleet: &Fleet, project: &str) -> Result<Vec<Place>> {
    let online = fleet.online();
    let mesh = fleet
        .mesh()
        .filter(|mesh| beep_fleet::has_project(mesh, project))
        .ok_or_else(|| DesktopError::BadRequest("The project is not in the mesh.".into()))?;
    let mut recorded: Vec<Place> = beep_fleet::places(&mesh, project)
        .into_iter()
        .filter(|(machine, _)| online.contains(machine))
        .map(|(machine, folder)| Place { machine, folder })
        .collect();
    beep_fleet::nearest_first(&mesh, &mut recorded, |place| place.machine);

    if recorded.is_empty() {
        return Err(DesktopError::BadRequest(
            "No machine with a checkout of the project is online.".into(),
        ));
    }

    Ok(recorded)
}

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

/// `project`'s branches, read at its nearest online checkout on a machine of the mesh.
pub(crate) async fn read_branches(fleet: &Fleet, project: String) -> Result<ProjectBranches> {
    // `placed` lists one at least.
    let Some(place) = placed(fleet, &project)?.into_iter().next() else {
        return Err(DesktopError::BadRequest(
            "No machine with a checkout of the project is online.".into(),
        ));
    };
    let (default_branch, branches) = direct::branches(fleet, &place, FALLBACK_DEFAULT).await?;
    Ok(ProjectBranches {
        machine_id: place.machine.to_string(),
        machine_name: fleet.name_of(place.machine),
        default_branch,
        branches,
    })
}

/// `project`'s branches.
#[tauri::command]
pub async fn project_branches(
    fleet: Machines<'_>,
    project: String,
) -> Result<Option<BranchesView>> {
    Ok(Some(read_branches(&fleet, project).await?.into()))
}

/// Gives `branch` of `project` a worktree on `machine_id`.
#[tauri::command]
pub async fn add_branch_worktree(
    fleet: Machines<'_>,
    project: String,
    branch: String,
    machine_id: String,
) -> Result<BranchRow> {
    let machine = fleet.machine(&machine_id)?;
    let place = placed(&fleet, &project)?
        .into_iter()
        .find(|place| place.machine == machine)
        .ok_or_else(|| {
            DesktopError::BadRequest(format!(
                "{} has no checkout of {project}.",
                fleet.name_of(machine)
            ))
        })?;
    let added = direct::add_worktree(&fleet, &place, &branch, FALLBACK_DEFAULT).await?;
    Ok(added.into())
}
