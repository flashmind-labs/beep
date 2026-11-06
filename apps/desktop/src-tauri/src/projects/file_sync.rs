//! How each machine stands with the projects it keeps in step, for the
//! window's machine table, a project's Machines tile and its File sync
//! view, which also pauses sync and settles conflicts.

use crate::{
    beep::Service,
    center::kept,
    center::view_cache::{View, ViewCache},
    error::Result,
    fleet::projects::{FleetState, desk, desk_or_server},
    projects::known,
};
use beep_projects::model::plain_path;
use beep_proto::{
    ConflictKeep, GetProjectSyncDetail, ListProjectSync, MachineSync, MachineSyncDetail,
    PauseProjectSync, ProjectSync, ProjectSyncDetail, SyncProgress, SyncStatus,
    node::sync::{FOLDER_MISSING, PARENT_MISSING},
};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

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

/// Every machine's sync of every project, one row each. Empty from a
/// server too old to sync projects.
#[tauri::command]
pub async fn project_sync(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    cache: State<'_, Arc<ViewCache>>,
) -> Result<Vec<SyncRow>> {
    let rows = match desk_or_server(&fleet, &beep).await {
        Ok(desk) => {
            let mut ours = match desk {
                Some(desk) => desk.sync().await,
                None => Vec::new(),
            };
            // The projects only the server syncs, until they are moved into the mesh.
            if beep.has_server() {
                match known(beep.call(ListProjectSync {}).await) {
                    Ok(theirs) => {
                        let theirs: Vec<ProjectSync> = theirs
                            .into_iter()
                            .flatten()
                            .filter(|one| !ours.iter().any(|ours| ours.project == one.project))
                            .collect();
                        ours.extend(theirs);
                    }
                    Err(e) => log::info!("the server's sync did not read: {e}"),
                }
            }
            Ok(rows(ours))
        }
        Err(e) => Err(e),
    };
    kept(&cache, View::Sync, rows)
}

/// A project's sync as the File sync view shows it.
#[derive(Debug, Serialize, PartialEq)]
pub struct SyncDetailView {
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
    progress: Option<ProgressRow>,
    waiting: Vec<String>,
    can_resolve: bool,
}

/// How far a machine is through what it brings in line now.
#[derive(Debug, Serialize, PartialEq)]
pub struct ProgressRow {
    files_done: u64,
    files_total: u64,
    bytes_done: u64,
    bytes_total: u64,
    /// Bytes a second; 0 before it can tell.
    rate: u64,
    eta_secs: Option<u32>,
}

impl From<SyncProgress> for ProgressRow {
    fn from(p: SyncProgress) -> Self {
        Self {
            files_done: p.files_done,
            files_total: p.files_total,
            bytes_done: p.bytes_done,
            bytes_total: p.bytes_total,
            rate: p.rate,
            eta_secs: p.eta_secs,
        }
    }
}

impl From<MachineSyncDetail> for MachineDetailRow {
    fn from(d: MachineSyncDetail) -> Self {
        let m = d.sync;
        Self {
            machine_id: m.machine_id,
            machine_name: m.machine_name,
            folder: plain_path(&m.folder).into_owned(),
            status: shown(m.status, m.error.as_deref()),
            files: m.files,
            bytes: m.bytes,
            pending: m.pending,
            conflicts: m.conflicts,
            error: m.error,
            hint: m.hint,
            paused: d.paused,
            progress: d.progress.map(Into::into),
            waiting: d.waiting,
            can_resolve: d.can_resolve,
        }
    }
}

impl From<ProjectSyncDetail> for SyncDetailView {
    fn from(d: ProjectSyncDetail) -> Self {
        Self {
            project: d.project,
            pinned: d.pinned,
            paused: d.paused,
            machines: d.machines.into_iter().map(Into::into).collect(),
        }
    }
}

/// `project`'s sync in detail, with no machines for a project none of them
/// sync; `None` from a server too old to say.
#[tauri::command]
pub async fn project_sync_detail(
    fleet: FleetState<'_>,
    beep: Service<'_>,
    project: String,
) -> Result<Option<SyncDetailView>> {
    let Some(desk) = desk_or_server(&fleet, &beep).await? else {
        return Ok(known(beep.call(GetProjectSyncDetail { project }).await)?.map(Into::into));
    };
    match desk.sync_detail(&project).await {
        Ok(detail) => Ok(Some(detail.into())),
        // Synced by the server alone, until the project is moved into the mesh.
        Err(beep_fleet::FleetError::NotFound) if beep.has_server() => {
            Ok(known(beep.call(GetProjectSyncDetail { project }).await)?.map(Into::into))
        }
        Err(beep_fleet::FleetError::NotFound) => Ok(Some(SyncDetailView {
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
    beep: Service<'_>,
    project: String,
    paused: bool,
) -> Result<SyncDetailView> {
    let Some(desk) = desk_or_server(&fleet, &beep).await? else {
        let detail = beep.call(PauseProjectSync { project, paused }).await?;
        return Ok(detail.into());
    };
    match desk.pause_sync(&project, paused).await {
        Ok(detail) => Ok(detail.into()),
        Err(beep_fleet::FleetError::NotFound) if beep.has_server() => {
            let detail = beep.call(PauseProjectSync { project, paused }).await?;
            Ok(detail.into())
        }
        Err(e) => Err(e.into()),
    }
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
mod tests {
    use super::*;

    fn machine(id: &str, status: SyncStatus) -> MachineSync {
        MachineSync {
            machine_id: id.into(),
            machine_name: id.into(),
            folder: "/srv/project".into(),
            status,
            files: 142,
            bytes: 0,
            pending: 468,
            conflicts: Vec::new(),
            rescan_secs: None,
            hint: None,
            error: None,
        }
    }

    #[test]
    fn each_machine_of_each_project_is_a_row_with_its_status_named() {
        let rows = rows(vec![ProjectSync {
            project: "acme/widget".into(),
            pinned: false,
            machines: vec![
                machine("m1", SyncStatus::Syncing {}),
                machine("m2", SyncStatus::InSync {}),
            ],
        }]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].project, "acme/widget");
        assert_eq!(rows[0].status, "syncing");
        assert_eq!(rows[1].status, "in_sync");
        assert_eq!(rows[1].machine_id, "m2");
        assert_eq!(rows[0].pending, 468);
    }

    #[test]
    fn the_detail_keeps_each_machine_s_progress_and_what_it_can_do() {
        let view: SyncDetailView = ProjectSyncDetail {
            project: "acme/widget".into(),
            pinned: false,
            paused: true,
            machines: vec![MachineSyncDetail {
                sync: machine("m1", SyncStatus::Syncing {}),
                paused: true,
                progress: Some(SyncProgress {
                    files_done: 142,
                    files_total: 610,
                    bytes_done: 48,
                    bytes_total: 210,
                    rate: 2,
                    eta_secs: Some(60),
                }),
                waiting: vec!["src/main.rs".into()],
                can_resolve: true,
            }],
        }
        .into();
        assert!(view.paused);
        let m = &view.machines[0];
        assert_eq!(m.status, "syncing");
        assert!(m.paused && m.can_resolve);
        assert_eq!(m.progress.as_ref().map(|p| p.files_total), Some(610));
        assert_eq!(m.waiting, ["src/main.rs"]);
    }

    #[test]
    fn a_failure_says_which_when_the_node_said_why() {
        let missing = format!("{FOLDER_MISSING}: /srv/project");
        assert_eq!(shown(SyncStatus::Failed {}, Some(&missing)), "missing");
        let unmounted = format!("{PARENT_MISSING}: /mnt/disk/project");
        assert_eq!(
            shown(SyncStatus::Failed {}, Some(&unmounted)),
            "parent_missing"
        );
        let held = "Already synced by Syncthing (share)";
        assert_eq!(shown(SyncStatus::Failed {}, Some(held)), "syncthing");
        assert_eq!(shown(SyncStatus::Failed {}, Some("no room")), "failed");
        assert_eq!(shown(SyncStatus::InSync {}, None), "in_sync");
    }
}
