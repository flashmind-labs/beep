use super::*;

#[test]
fn only_a_stopped_session_in_a_folder_that_exists_is_taken_over() {
    let mut found = ExternalSession {
        id: "0f3c2a9e-1111-4222-8333-944455556666".into(),
        cwd: "/".into(),
        presence: Presence::Working,
        pid: Some(41),
        ..ExternalSession::default()
    };
    let refused = refuse_takeover(&found).unwrap();
    assert!(refused.contains("can only follow it"), "{refused}");
    assert!(refused.contains("process 41"), "{refused}");
    found.presence = Presence::Recent;
    found.pid = None;
    let refused = refuse_takeover(&found).unwrap();
    assert!(refused.contains("was active a moment ago"), "{refused}");
    found.presence = Presence::Stopped;
    assert_eq!(refuse_takeover(&found), None);
    found.cwd = "/no/such/folder/here".into();
    assert!(refuse_takeover(&found).is_some());
}

#[test]
fn the_config_dir_follows_the_variable_then_home() {
    let home = Some(PathBuf::from("/home/user"));
    let usual = Some(PathBuf::from("/home/user/.claude"));
    assert_eq!(config_dir_in(None, home.clone()), usual);
    assert_eq!(config_dir_in(Some("".into()), home.clone()), usual);
    let set = config_dir_in(Some("/srv/claude".into()), home);
    assert_eq!(set, Some(PathBuf::from("/srv/claude")));
}

#[tokio::test]
async fn sessions_show_once_the_projects_folder_appears() {
    let id = "0f3c2a9e-1111-4222-8333-944455556666";
    let root = tempfile::tempdir().expect("a temp folder");
    let external = External::locate_in(root.path());
    let listed = external.list(10, None, HashMap::new()).await;
    assert!(listed.expect("an empty list").is_empty());
    let project = root.path().join("projects/-srv-project");
    std::fs::create_dir_all(&project).expect("folders");
    let line = r#"{"type":"user","cwd":"/srv/project","timestamp":"2026-11-02T10:00:00.000Z","message":{"role":"user","content":"hi"}}"#;
    std::fs::write(project.join(format!("{id}.jsonl")), format!("{line}\n")).expect("a transcript");
    let listed = external.list(10, None, HashMap::new()).await;
    assert_eq!(listed.expect("a list").len(), 1);
}
