use super::*;

#[test]
fn restores_only_open_windows_and_keeps_a_closed_windows_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("windows.json");
    let store = WindowSession::load(path.clone());
    let project = Alone::Project {
        project: "sample".into(),
    };
    let chat = Alone::Chat {
        machine_id: "build-box".into(),
        session: "chat".into(),
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let desk = serde_json::json!({ "index": 1, "name": "Work" });
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let desk = serde_json::json!("7cb551a8-9b50-4050-afa2-af5ff6b57ddd");
    store.record(
        &project.label(),
        true,
        Some(serde_json::from_value(desk.clone()).unwrap()),
    );
    store.record(&chat.label(), true, None);
    store.record(&project.label(), false, None);
    lock(&store.session).quitting = true;
    store.record(&chat.label(), false, None);

    let restored = WindowSession::load(path);
    assert_eq!(restored.open_windows(), vec![chat]);
    let kept = lock(&restored.session);
    assert_eq!(
        serde_json::to_value(&kept.windows[&project.label()].desktop).unwrap(),
        desk
    );
}
