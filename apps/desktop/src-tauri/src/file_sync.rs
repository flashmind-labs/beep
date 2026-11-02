//! How each machine stands with the projects it keeps in step, for the
//! window's machine table and a project's Machines tile.

use crate::{beep::Beep, error::Result, projects::unknown_op};
use beep_proto::{ListProjectSync, MachineSync, ProjectSync};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

/// One machine's sync of one project.
#[derive(Debug, Serialize, PartialEq)]
pub struct SyncRow {
    project: String,
    machine_id: String,
    /// `offline`, `scanning`, `syncing`, `in_sync` or `failed`.
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
            status: m.status.into(),
            files: m.files,
            pending: m.pending,
            error: m.error,
        }
    }
}

/// Every machine's sync of every project, one row each. Empty from a
/// server too old to sync projects.
#[tauri::command]
pub async fn project_sync(beep: State<'_, Arc<Beep>>) -> Result<Vec<SyncRow>> {
    match beep.call(ListProjectSync {}).await {
        Ok(projects) => Ok(rows(projects)),
        Err(e) if unknown_op(&e) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
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
    use beep_proto::SyncStatus;

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
}
