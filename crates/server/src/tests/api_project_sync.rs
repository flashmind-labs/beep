use super::*;

fn row() -> SyncRow {
    SyncRow {
        project: "game".into(),
        endpoint: "node".into(),
        folder: "/srv/game".into(),
    }
}

#[test]
fn a_paused_machine_is_quiet_and_its_progress_carries_over() {
    // An older node stopped the paused project: not a refusal.
    let stopped = machine_detail(&row(), "mac".into(), Some(None), true);
    assert_eq!(stopped.sync.status, SyncStatus::InSync {});
    assert_eq!(stopped.sync.error, None);
    assert!(stopped.paused);
    let offline = machine_detail(&row(), "mac".into(), None, true);
    assert_eq!(offline.sync.status, SyncStatus::Offline {});
    assert!(!offline.paused);

    let paused = ProjectStatus {
        name: "game".into(),
        state: State::Paused,
        paused: true,
        ..ProjectStatus::default()
    };
    let detail = machine_detail(&row(), "mac".into(), Some(Some(&paused)), true);
    assert_eq!(detail.sync.status, SyncStatus::InSync {});
    assert!(detail.paused);

    let moving = ProjectStatus {
        name: "game".into(),
        state: State::Syncing,
        progress: Some(Progress {
            files_done: 142,
            files_total: 610,
            bytes_done: 48_000_000,
            bytes_total: 210_000_000,
            rate: 2_100_000,
            eta_secs: Some(78),
        }),
        waiting: vec!["src/main.rs".into()],
        ..ProjectStatus::default()
    };
    let detail = machine_detail(&row(), "mac".into(), Some(Some(&moving)), false);
    assert_eq!(detail.sync.status, SyncStatus::Syncing {});
    assert!(!detail.paused);
    let progress = detail.progress.expect("progress");
    assert_eq!((progress.files_done, progress.files_total), (142, 610));
    assert_eq!(progress.rate, 2_100_000);
    assert_eq!(progress.eta_secs, Some(78));
    assert_eq!(detail.waiting, ["src/main.rs"]);
}
