use super::*;
use crate::palette_store::{
    PaletteStore,
    lists::{BranchRow, PaletteLists},
    places::Place,
};
use crate::store::view_cache::{View, ViewCache};
use beep_proto::ImportedProject;
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
    let cache = PaletteCache::load(&PaletteStore::load(&upgraded))?;
    let index: serde_json::Value = serde_json::from_str(&lock(&cache.index))?;
    assert_eq!(index["projects"][0]["id"], "acme/widget");
    assert_eq!(index["projects"][0]["keys"][0], "acme/widget");
    Ok(())
}
