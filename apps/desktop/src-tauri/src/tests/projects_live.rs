use super::*;

#[tokio::test]
async fn selected_run_survives_project_registration_that_finishes_later() {
    let followed = Followed::default();
    let project = Uuid::new_v4();
    let (stop, _) = tokio::sync::oneshot::channel();
    followed.select("one", project, Some(SelectedRun { id: 42, host: None }));
    followed.add("one", project, || true, || source(stop));
    assert_eq!(
        followed
            .selected(project)
            .iter()
            .map(|run| run.id)
            .collect::<Vec<_>>(),
        [42]
    );
}

#[tokio::test]
async fn hidden_project_drops_a_selection_waiting_for_registration() {
    let followed = Followed::default();
    let project = Uuid::new_v4();
    followed.select("one", project, Some(SelectedRun { id: 42, host: None }));
    followed.clear("one");
    let (stop, _) = tokio::sync::oneshot::channel();
    followed.add("one", project, || true, || source(stop));
    assert!(followed.selected(project).is_empty());
    followed.remove("one", Some(project));
    followed.select("one", project, Some(SelectedRun { id: 42, host: None }));
    followed.select("one", project, None);
    let (stop, _) = tokio::sync::oneshot::channel();
    followed.add("one", project, || true, || source(stop));
    assert!(followed.selected(project).is_empty());
}
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::oneshot;

fn source(stopped: oneshot::Sender<()>) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let _stopped = stopped;
        std::future::pending::<()>().await;
    })
}

#[tokio::test]
async fn windows_share_a_source_until_its_last_owner_leaves() {
    let followed = Followed::default();
    let project = Uuid::new_v4();
    let (stop, stopped) = oneshot::channel();
    followed.add("one", project, || true, || source(stop));
    followed.add("two", project, || true, || panic!("duplicate source"));
    {
        let mut tasks = followed.tasks.lock().unwrap();
        let following = &mut tasks[0];
        following
            .selected
            .insert("one".into(), SelectedRun { id: 1, host: None });
        following.selected.insert(
            "two".into(),
            SelectedRun {
                id: 2,
                host: Some("code.example".into()),
            },
        );
    }
    assert_eq!(followed.selected(project).len(), 2);
    followed.remove("one", Some(project));
    {
        let tasks = followed.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].windows, HashSet::from(["two".to_owned()]));
        assert_eq!(tasks[0].selected.len(), 1);
    }
    assert_eq!(followed.selected(project)[0].id, 2);
    followed.clear("two");
    assert!(followed.tasks.lock().unwrap().is_empty());
    assert!(followed.selected(project).is_empty());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), stopped)
            .await
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn clearing_one_window_keeps_other_windows_project_sources() {
    let followed = Followed::default();
    let shared = Uuid::new_v4();
    let other = Uuid::new_v4();
    let (shared_stop, _shared_stopped) = oneshot::channel();
    let (other_stop, other_stopped) = oneshot::channel();
    followed.add("one", shared, || true, || source(shared_stop));
    followed.add("two", shared, || true, || panic!("duplicate source"));
    followed.add("one", other, || true, || source(other_stop));
    followed.clear("one");
    {
        let tasks = followed.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].project_id, shared);
        assert!(tasks[0].windows.contains("two"));
    }
    assert!(
        tokio::time::timeout(Duration::from_secs(1), other_stopped)
            .await
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn a_destroyed_window_cannot_leave_a_late_source_registration() {
    let followed = Followed::default();
    followed.add("gone", Uuid::new_v4(), || false, || panic!("closed window"));
    let checks = AtomicUsize::new(0);
    let (stop, stopped) = oneshot::channel();
    followed.add(
        "closing",
        Uuid::new_v4(),
        || checks.fetch_add(1, Ordering::SeqCst) == 0,
        || source(stop),
    );
    assert!(followed.tasks.lock().unwrap().is_empty());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), stopped)
            .await
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn one_windows_limit_cannot_stop_another_windows_source() {
    let followed = Followed::default();
    let shared = Uuid::new_v4();
    let (stop, _stopped) = oneshot::channel();
    followed.add("one", shared, || true, || source(stop));
    followed.add("two", shared, || true, || panic!("duplicate source"));

    for _ in 0..MAX_FOLLOWED {
        let (stop, _stopped) = oneshot::channel();
        followed.add("one", Uuid::new_v4(), || true, || source(stop));
    }

    let tasks = followed.tasks.lock().unwrap();
    let shared = tasks.iter().find(|task| task.project_id == shared).unwrap();
    assert_eq!(shared.windows, HashSet::from(["two".to_owned()]));
    assert_eq!(
        tasks
            .iter()
            .filter(|task| task.windows.contains("one"))
            .count(),
        MAX_FOLLOWED
    );
}
