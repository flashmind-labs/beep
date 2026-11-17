use super::*;
use std::time::SystemTime;

#[test]
fn removing_a_project_clears_its_lists_and_rejects_late_session_lists() {
    let project = ImportedProject {
        project: "acme/widget".into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        folder: "/srv/project".into(),
        url: "https://example.com/acme/widget".into(),
        cloned: false,
        imported_at: SystemTime::UNIX_EPOCH,
        display_name: None,
    };
    let session = MachineSession {
        reply: None,
        changes: None,
        id: "a".into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        agent: "codex".into(),
        title: Some("Retry fixes".into()),
        cwd: "/srv/project/.worktrees/fix".into(),
        status: beep_proto::AgentStatus::Ended {},
        doing: None,
        question: None,
        subagents: 0,
        model: None,
        external: false,
        started_at: SystemTime::UNIX_EPOCH,
        last_activity: SystemTime::UNIX_EPOCH,
        assistant: false,
    };
    let mut lists = PaletteLists {
        projects: Some(vec![project.clone()]),
        sessions: Some(vec![session.clone()]),
        branches: Some(vec![BranchRow {
            project: project.project.clone(),
            name: "fix".into(),
            ahead: 1,
            subject: "Retry".into(),
        }]),
        retired_places: vec![Place::from(&project)],
        forgotten_projects: [project.project.clone()].into(),
        ..PaletteLists::default()
    };
    lists.prune();
    let index = serde_json::to_value(build_index(&lists)).expect("the index serializes");

    for group in ["projects", "sessions", "code"] {
        assert_eq!(index[group], serde_json::json!([]));
    }

    lists.sessions = Some(vec![session]);
    lists.prune();
    let index = serde_json::to_value(build_index(&lists)).expect("the index serializes");
    assert_eq!(
        index["projects"],
        serde_json::json!([]),
        "a stale session cannot return as an unimported folder"
    );
    assert_eq!(index["sessions"], serde_json::json!([]));
}

#[test]
fn cached_projects_are_searchable_before_any_machine_answers() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("views.json");
    let views = ViewCache::load(path.clone(), "0.2.0");
    let lists = PaletteLists {
        projects: Some(vec![ImportedProject {
            project: "acme/widget".into(),
            url: "https://example.com/acme/widget".into(),
            machine_id: "build-box".into(),
            machine_name: "build-box".into(),
            folder: "/srv/project".into(),
            cloned: false,
            imported_at: SystemTime::UNIX_EPOCH,
            display_name: None,
        }]),
        ..PaletteLists::default()
    };
    views.put(View::Palette, serde_json::to_value(lists)?);
    views.write_unsaved();
    let upgraded = ViewCache::load(path, "0.2.1");
    let cache = PaletteCache::load(&upgraded)?;
    let index: serde_json::Value = serde_json::from_str(&cache.held().index)?;
    assert_eq!(index["projects"][0]["id"], "acme/widget");
    assert_eq!(index["projects"][0]["keys"][0], "acme/widget");
    Ok(())
}

/// A list that does not survive the file would leave the next launch
/// empty, and only log it.
#[test]
fn lists_come_back_from_the_views_file() {
    let lists = PaletteLists {
        machines: Some(vec![Machine {
            id: "b".to_owned(),
            name: "build-box".to_owned(),
            online: true,
            seen_at: SystemTime::UNIX_EPOCH,
            version: String::new(),
            env: None,
        }]),
        ..PaletteLists::default()
    };
    let saved = serde_json::to_value(&lists).expect("the lists are plain data");
    let back: PaletteLists = serde_json::from_value(saved).expect("they read back");
    assert_eq!(back.machines.expect("kept")[0].name, "build-box");
}

#[test]
fn a_place_that_left_stays_out_until_imported_again() {
    let at = |project: &str, folder: &str| ImportedProject {
        project: project.into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        folder: folder.into(),
        url: String::new(),
        cloned: false,
        imported_at: SystemTime::UNIX_EPOCH,
        display_name: None,
    };
    let widget = at("acme/widget", "/srv/project");
    let tools = at("acme/tools", "/srv/tools");
    let mut lists = PaletteLists::default();
    lists.set_projects(vec![widget.clone(), tools.clone()]);
    assert!(
        lists.retired_places.is_empty(),
        "the first list retires nothing"
    );

    lists.set_projects(vec![tools.clone()]);
    assert_eq!(lists.retired_places, [Place::from(&widget)]);
    assert!(lists.forgotten_projects.contains("acme/widget"));

    lists.set_projects(vec![widget.clone(), tools.clone()]);
    assert!(lists.retired_places.is_empty(), "imported again");
    assert!(lists.forgotten_projects.is_empty());

    lists.forget_project("acme/tools");
    lists.forget_project("acme/tools");
    assert_eq!(lists.retired_places, [Place::from(&tools)], "once");
    assert!(lists.forgotten_projects.contains("acme/tools"));
}
