//! How each machine stands with the projects it keeps in step, for the
//! window's machine table, a project's Machines tile and its File sync
//! view, which also pauses sync and settles conflicts.

use crate::{
    error::Result,
    fleet::{
        Machines,
        projects::{FleetState, desk},
    },
    store::kept,
    store::view_cache::{View, ViewCache},
};
use beep_fleet::Desk;
use beep_projects::model::plain_path;
use beep_proto::{
    ConflictKeep, MachineSync, MachineSyncDetail, ProjectSync, ProjectSyncDetail, SyncProgress,
    SyncStatus,
    node::sync::{FOLDER_CHANGED, FOLDER_MISSING, PARENT_MISSING},
};
use serde::Serialize;
use std::{collections::BTreeSet, sync::Arc};
use tauri::State;

#[tauri::command]
pub async fn project_git_auto_recovery(
    fleet: FleetState<'_>,
    project: String,
    enabled: Option<bool>,
) -> Result<bool> {
    Ok(desk(&fleet)
        .await?
        .project_git_auto_recovery(&project, enabled)
        .await?)
}

#[tauri::command]
pub async fn project_syncthing_mode(
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    project: String,
    enabled: Option<bool>,
) -> Result<Option<bool>> {
    let result = desk(&fleet)
        .await?
        .project_syncthing_mode(&project, enabled)
        .await?;
    if enabled.is_some() {
        machines.projections().forget_syncthing(&project);
    }
    Ok(result)
}

#[tauri::command]
pub async fn project_ignored_sync(
    fleet: FleetState<'_>,
    project: String,
    settings: Option<beep_proto::node::sync::IgnoredSync>,
) -> Result<beep_proto::node::sync::IgnoredSync> {
    Ok(desk(&fleet)
        .await?
        .project_ignored_sync(&project, settings)
        .await?)
}

#[tauri::command]
pub async fn project_sync_rules(
    fleet: FleetState<'_>,
    project: String,
    rules: Option<String>,
) -> Result<String> {
    Ok(desk(&fleet)
        .await?
        .project_sync_rules(&project, rules)
        .await?)
}

#[tauri::command]
pub async fn project_handoff_work(
    fleet: FleetState<'_>,
    project: String,
    from: String,
    to: String,
) -> Result<()> {
    Ok(desk(&fleet)
        .await?
        .handoff_work(&project, &from, &to)
        .await?)
}

/// Whether any machine runs Syncthing, so the add sheets offer its choice.
#[tauri::command]
pub async fn syncthing_in_use(fleet: FleetState<'_>) -> Result<bool> {
    Ok(desk(&fleet).await?.syncthing_in_use().await)
}

#[tauri::command]
pub async fn project_syncthing_files(
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    machine_id: String,
    folder: String,
    enabled: Option<bool>,
) -> Result<beep_proto::node::sync::SyncthingFiles> {
    let result = desk(&fleet)
        .await?
        .syncthing_files(&machine_id, &folder, enabled)
        .await?;
    if enabled.is_some() {
        machines.projections().forget_all_syncthing();
    }
    Ok(result)
}

/// One machine's sync of one project.
#[derive(Debug, Serialize, PartialEq)]
pub struct SyncRow {
    project: String,
    machine_id: String,
    /// `offline`, `scanning`, `syncing`, `in_sync`, `failed`, or as `failed`, with why:
    /// `missing` (the node makes the folder again), `parent_missing` (a disk not mounted, say)
    /// or `syncthing` (Syncthing keeps the folder in step).
    status: &'static str,
    /// Files kept in step, and files another machine changed that this one
    /// does not have yet.
    files: u64,
    pending: u64,
    error: Option<String>,
}

impl SyncRow {
    fn new(project: &str, m: MachineSync) -> Self {
        Self {
            project: project.to_owned(),
            machine_id: m.machine_id,
            status: shown(m.status, m.error.as_deref()),
            files: m.files,
            pending: m.pending,
            error: m.error,
        }
    }
}

/// The status a machine's row shows: a failure says which, when the node said why.
fn shown(status: SyncStatus, error: Option<&str>) -> &'static str {
    let failed: &'static str = status.into();

    if failed != "failed" {
        return failed;
    }

    match error {
        Some(why) if why.contains(PARENT_MISSING) => "parent_missing",
        Some(why) if why.contains(FOLDER_MISSING) => "missing",
        Some(why) if why.starts_with(SYNCTHING) => "syncthing",
        _ => failed,
    }
}

/// How a node says Syncthing keeps the folder, so Beep's sync waits.
const SYNCTHING: &str = "Already synced by Syncthing";

/// Every machine's sync of every project, one row each.
#[tauri::command]
pub async fn project_sync(
    fleet: FleetState<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Vec<SyncRow>> {
    let rows = match desk(&fleet).await {
        Ok(desk) => Ok(rows(desk.sync().await)),
        Err(e) => Err(e),
    };
    kept(&cache, View::Sync, rows)
}

/// The projects of `rows` with a machine copying or scanning: the ones whose detail Home shows.
pub(crate) fn unsettled(rows: &[SyncRow]) -> BTreeSet<String> {
    rows.iter()
        .filter(|row| matches!(row.status, "syncing" | "scanning"))
        .map(|row| row.project.clone())
        .collect()
}

/// A project's sync as the File sync view shows it.
#[derive(Debug, Serialize, PartialEq)]
pub struct SyncDetailView {
    ignored_folders: Vec<String>,
    project: String,
    pinned: bool,
    paused: bool,
    machines: Vec<MachineDetailRow>,
}

/// One machine's sync of the project, in detail.
#[derive(Debug, Serialize, PartialEq)]
pub struct MachineDetailRow {
    machine_id: String,
    machine_name: String,
    folder: String,
    /// As `SyncRow::status`.
    status: &'static str,
    files: u64,
    bytes: u64,
    pending: u64,
    conflicts: Vec<String>,
    error: Option<String>,
    /// What raises the file watch limit, while the folder is rescanned.
    hint: Option<String>,
    paused: bool,
    progress: Option<SyncProgress>,
    waiting: Vec<String>,
    git_only: bool,
    can_resolve: bool,
    /// The folder at its path is not the one synced before, as when it was
    /// replaced, and its beep-node can start over with it.
    can_start_over: bool,
}

impl From<MachineSyncDetail> for MachineDetailRow {
    fn from(d: MachineSyncDetail) -> Self {
        let m = d.sync;
        let can_start_over = d.can_resolve
            && m.error
                .as_deref()
                .is_some_and(|error| error.contains(FOLDER_CHANGED));
        Self {
            machine_id: m.machine_id,
            machine_name: m.machine_name,
            folder: plain_path(&m.folder).into_owned(),
            status: shown(m.status, m.error.as_deref()),
            git_only: m.git_only,
            files: m.files,
            bytes: m.bytes,
            pending: m.pending,
            conflicts: m.conflicts,
            error: m.error,
            hint: m.hint,
            paused: d.paused,
            progress: d.progress,
            waiting: d.waiting,
            can_resolve: d.can_resolve,
            can_start_over,
        }
    }
}

impl From<ProjectSyncDetail> for SyncDetailView {
    fn from(d: ProjectSyncDetail) -> Self {
        Self {
            ignored_folders: Vec::new(),
            project: d.project,
            pinned: d.pinned,
            paused: d.paused,
            machines: d.machines.into_iter().map(Into::into).collect(),
        }
    }
}

/// `project`'s sync in detail, with no machines for a project none of them
/// sync.
#[tauri::command]
pub async fn project_sync_detail(
    fleet: FleetState<'_>,
    project: String,
) -> Result<Option<SyncDetailView>> {
    sync_detail_of(&desk(&fleet).await?, project).await
}

/// `project_sync_detail` for a project of `desk`, as the sync view pushes it too.
pub(crate) async fn sync_detail_of(desk: &Desk, project: String) -> Result<Option<SyncDetailView>> {
    match desk.sync_detail(&project).await {
        Ok(detail) => {
            let mut view: SyncDetailView = detail.into();

            if let Ok(settings) = desk.project_ignored_sync(&project, None).await
                && settings.enabled
            {
                view.ignored_folders = settings.folders;
            }

            Ok(Some(view))
        }
        Err(beep_fleet::FleetError::NotFound) => Ok(Some(SyncDetailView {
            ignored_folders: Vec::new(),
            project,
            pinned: false,
            paused: false,
            machines: Vec::new(),
        })),
        Err(e) => Err(e.into()),
    }
}

/// Pauses `project`'s sync on every machine, or resumes it.
#[tauri::command]
pub async fn project_sync_pause(
    fleet: FleetState<'_>,
    project: String,
    paused: bool,
) -> Result<SyncDetailView> {
    let detail = desk(&fleet).await?.pause_sync(&project, paused).await?;
    Ok(detail.into())
}

/// Settles the conflict copy at `path` on `machine_id`: keeps the copy in
/// place of the file beside it, or keeps that file and deletes the copy.
#[tauri::command]
pub async fn project_sync_resolve(
    fleet: FleetState<'_>,
    project: String,
    machine_id: String,
    path: String,
    keep_copy: bool,
) -> Result<()> {
    let keep = if keep_copy {
        ConflictKeep::Copy {}
    } else {
        ConflictKeep::Original {}
    };
    Ok(desk(&fleet)
        .await?
        .resolve_conflict(&project, &machine_id, &path, keep)
        .await?)
}

/// Starts `project`'s sync over on `machine_id` in the folder now at its
/// path, as for one that was replaced.
#[tauri::command]
pub async fn project_sync_start_over(
    fleet: FleetState<'_>,
    project: String,
    machine_id: String,
) -> Result<()> {
    Ok(desk(&fleet)
        .await?
        .start_sync_over(&project, &machine_id)
        .await?)
}

fn rows(projects: Vec<ProjectSync>) -> Vec<SyncRow> {
    projects
        .into_iter()
        .flat_map(|p| {
            let ProjectSync {
                project, machines, ..
            } = p;
            machines.into_iter().map(move |m| SyncRow::new(&project, m))
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/projects_file_sync.rs"]
mod tests;
