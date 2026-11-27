use super::*;

#[tokio::test]
async fn sessions_show_once_the_projects_folder_appears() {
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let root = tempfile::tempdir().expect("a temp folder");
    let store = Store::locate_in(root.path());
    let listed = store.list(10, None, HashMap::new()).await;
    assert!(listed.expect("an empty list").is_empty());
    let project = root.path().join("projects/-srv-project");
    std::fs::create_dir_all(&project).expect("folders");
    let line = r#"{"type":"user","cwd":"/srv/project","timestamp":"2026-11-02T10:00:00.000Z","message":{"role":"user","content":"hi"}}"#;
    std::fs::write(project.join(format!("{id}.jsonl")), format!("{line}\n")).expect("a transcript");
    let listed = store.list(10, None, HashMap::new()).await;
    assert_eq!(listed.expect("a list").len(), 1);
}

#[tokio::test]
async fn a_follow_reads_the_processes_again_only_when_stale_or_old() {
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let root = tempfile::tempdir().expect("a temp folder");
    let project = root.path().join("projects/-srv-project");
    std::fs::create_dir_all(&project).expect("folders");
    let line = r#"{"type":"user","cwd":"/srv/project","timestamp":"2026-11-02T10:00:00.000Z","message":{"role":"user","content":"hi"}}"#;
    std::fs::write(project.join(format!("{id}.jsonl")), format!("{line}\n")).expect("a transcript");
    let store = Store::locate_in(root.path());
    let mut followed = store
        .follow(&SessionId::from(id))
        .await
        .expect("found")
        .expect("a file");
    let read_at = |followed: &Followed| followed.machine.as_ref().map(|(_, at)| *at);

    // No process runs it, and the first read is kept all the same.
    let found = store.followed(&mut followed, false, HashMap::new()).await;
    assert_eq!(found.expect("the session").pid, None);
    let first = read_at(&followed).expect("a reading");
    store
        .followed(&mut followed, false, HashMap::new())
        .await
        .expect("the session");
    assert_eq!(read_at(&followed), Some(first));

    store
        .followed(&mut followed, true, HashMap::new())
        .await
        .expect("the session");
    let second = read_at(&followed).expect("a reading");
    assert_ne!(second, first);

    let old = second.checked_sub(MACHINE_KEPT).expect("a past instant");
    followed.machine.as_mut().expect("a reading").1 = old;
    store
        .followed(&mut followed, false, HashMap::new())
        .await
        .expect("the session");
    assert!(read_at(&followed).is_some_and(|at| at > old));
}
