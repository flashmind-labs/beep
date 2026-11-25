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
        account: None,
        persona: None,
        place: None,
        continues: None,
    };
    let mut lists = PaletteLists {
        projects: Some(vec![project.clone()]),
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
    let mut sessions = vec![session];
    lists.without_retired(&mut sessions);
    let index = serde_json::to_value(build_index(&lists, &sessions)).expect("the index serializes");

    for group in ["projects", "sessions", "code"] {
        assert_eq!(index[group], serde_json::json!([]));
    }
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

#[test]
fn a_read_taken_before_a_removal_does_not_bring_the_project_back() {
    let project = |name: &str| ImportedProject {
        project: name.into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        folder: format!("/srv/{name}"),
        url: format!("https://example.com/{name}"),
        cloned: false,
        imported_at: SystemTime::UNIX_EPOCH,
        display_name: None,
    };
    let mut lists = PaletteLists::default();

    assert!(lists.read_projects(1, vec![project("acme/widget")]));
    // The removal is ticket 3; the read with ticket 2 started before it and still holds the project.
    lists.removed_at = 3;
    lists.forget_project("acme/widget");
    assert!(!lists.read_projects(2, vec![project("acme/widget")]));
    assert!(lists.forgotten_projects.contains("acme/widget"));

    // A read that started after it saw the removal.
    assert!(lists.read_projects(4, Vec::new()));
    assert!(lists.projects.as_deref().unwrap().is_empty());
}

#[test]
fn a_read_older_than_the_one_held_is_dropped() {
    let project = |name: &str| ImportedProject {
        project: name.into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        folder: format!("/srv/{name}"),
        url: format!("https://example.com/{name}"),
        cloned: false,
        imported_at: SystemTime::UNIX_EPOCH,
        display_name: None,
    };
    let mut lists = PaletteLists::default();

    assert!(lists.read_projects(3, vec![project("acme/other")]));
    assert!(lists.read_projects(5, Vec::new()));
    // A read that started earlier ends later and must not undo the newer one.
    assert!(!lists.read_projects(4, vec![project("acme/widget")]));
    assert!(lists.projects.as_deref().unwrap().is_empty());
}

fn opened_session(id: &str, cwd: &str) -> MachineSession {
    MachineSession {
        reply: None,
        changes: None,
        id: id.into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        agent: "codex".into(),
        title: Some(format!("Session {id}")),
        cwd: cwd.into(),
        status: beep_proto::AgentStatus::Ended {},
        doing: None,
        question: None,
        subagents: 0,
        model: None,
        external: false,
        started_at: SystemTime::UNIX_EPOCH,
        last_activity: SystemTime::UNIX_EPOCH,
        assistant: false,
        account: None,
        persona: None,
        place: None,
        continues: None,
    }
}

#[test]
fn a_session_the_user_opened_is_kept_once_few_and_across_launches() {
    let mut lists = PaletteLists::default();
    lists.open(opened_session("a", "/srv/one"));
    lists.open(opened_session("a", "/srv/two"));
    assert_eq!(lists.opened.len(), 1, "one row for a session");
    assert_eq!(lists.opened[0].cwd, "/srv/two");

    for n in 0..OPENED_MOST {
        lists.open(opened_session(&format!("s{n}"), "/srv/one"));
    }
    assert_eq!(lists.opened.len(), OPENED_MOST);
    assert!(
        lists.opened.iter().all(|s| s.id.as_ref() != "a"),
        "the oldest goes"
    );

    let saved = serde_json::to_value(&lists).expect("the lists are plain data");
    let back: PaletteLists = serde_json::from_value(saved).expect("they read back");
    assert_eq!(back.opened.len(), OPENED_MOST);
}

#[test]
fn a_session_in_a_place_that_left_is_not_kept_as_opened() {
    let place = ImportedProject {
        project: "acme/widget".into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        folder: "/srv/project".into(),
        url: "https://example.com/acme/widget".into(),
        cloned: false,
        imported_at: SystemTime::UNIX_EPOCH,
        display_name: None,
    };
    let mut lists = PaletteLists {
        // Listed too, it would be a place imported again, which is not retired.
        projects: Some(Vec::new()),
        retired_places: vec![Place::from(&place)],
        ..PaletteLists::default()
    };
    lists.open(opened_session("a", "/srv/project"));
    assert!(lists.opened.is_empty());
}
