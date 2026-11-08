use super::*;
use beep_proto::MemorySource;
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
    };
    let session = MachineSession {
        id: "a".into(),
        machine_id: "build-box".into(),
        machine_name: "build-box".into(),
        agent: "codex".into(),
        title: Some("Retry fixes".into()),
        cwd: "/srv/project/.worktrees/fix".into(),
        status: beep_proto::AgentStatus::Ended {},
        doing: None,
        question: None,
        external: false,
        started_at: SystemTime::UNIX_EPOCH,
        last_activity: SystemTime::UNIX_EPOCH,
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
        prs: Some(vec![PrRow {
            repo: project.project.clone(),
            number: 1,
            title: "Retry".into(),
            url: "https://example.com/pull/1".into(),
            detail: String::new(),
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
        }]),
        ..PaletteLists::default()
    };
    views.put(View::Palette, serde_json::to_value(lists)?);
    views.write_unsaved();
    views.keep_local();
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
        memories: Some(vec![Memory {
            id: "m".to_owned(),
            content: "likes tea".to_owned(),
            category: "facts".to_owned(),
            source: MemorySource::User {},
            updated: "2026-01-01".to_owned(),
            valid_from: None,
            valid_until: None,
            quote: None,
        }]),
        machines: Some(vec![Machine {
            id: "b".to_owned(),
            name: "build-box".to_owned(),
            online: true,
            seen_at: SystemTime::UNIX_EPOCH,
            version: String::new(),
            env: None,
        }]),
        chats: Some(Vec::new()),
        ..PaletteLists::default()
    };
    let saved = serde_json::to_value(&lists).expect("the lists are plain data");
    let back: PaletteLists = serde_json::from_value(saved).expect("they read back");
    assert_eq!(back.memories.expect("kept")[0].content, "likes tea");
    assert_eq!(back.machines.expect("kept")[0].name, "build-box");
    assert!(back.chats.is_none(), "chats have their own file");
}
