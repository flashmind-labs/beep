use super::*;

fn machine(id: &str, status: SyncStatus) -> MachineSync {
    MachineSync {
        git_only: false,
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
