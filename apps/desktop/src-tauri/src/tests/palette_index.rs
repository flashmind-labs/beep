use super::*;
use crate::palette_store::lists::{BranchRow, PaletteLists};
use beep_proto::SessionId;
use beep_proto::{AgentStatus, ImportedProject, MachineSession};
use std::time::{Duration, SystemTime};

fn imported(project: &str, machine: &str) -> ImportedProject {
    ImportedProject {
        project: project.to_owned(),
        url: format!("https://example.com/{project}"),
        machine_id: machine.to_owned(),
        machine_name: machine.to_owned(),
        folder: "/srv/project".to_owned(),
        cloned: false,
        imported_at: SystemTime::UNIX_EPOCH,
        display_name: None,
    }
}

fn agent_session(
    id: &str,
    title: Option<&str>,
    cwd: &str,
    machine: &str,
    at: u64,
) -> MachineSession {
    MachineSession {
        reply: None,
        changes: None,
        machine_id: machine.to_owned(),
        machine_name: machine.to_owned(),
        id: SessionId::from(id),
        agent: "claude".to_owned(),
        title: title.map(str::to_owned),
        cwd: cwd.to_owned(),
        status: AgentStatus::Working {},
        doing: None,
        started_at: SystemTime::UNIX_EPOCH,
        last_activity: SystemTime::UNIX_EPOCH + Duration::from_secs(at),
        external: false,
        question: None,
        subagents: 0,
        model: None,
        assistant: false,
        account: None,
        persona: None,
        place: None,
        continues: None,
    }
}

fn titles(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.title.as_str()).collect()
}

#[test]
fn the_assistants_pages_are_not_found() {
    let index = build_index(&PaletteLists::default(), &[]);
    let actions = titles(&index.actions);
    for gone in [
        "New chat",
        "All chats",
        "Memories",
        "Routines",
        "Skills",
        "Apps",
    ] {
        assert!(!actions.contains(&gone), "{gone}");
    }
    for kept in [
        "New session",
        "Home",
        "Projects",
        "Terminals",
        "Add project",
        "Join team",
        "Add machine",
        "Settings",
        "Coding agents",
        "Keyboard shortcuts",
        "Report a problem",
    ] {
        assert!(actions.contains(&kept), "{kept}");
    }

    let settings = titles(&index.settings);
    for gone in [
        "Account",
        "Model",
        "Apps",
        "Let Beep use this Mac",
        "Agent accounts",
    ] {
        assert!(!settings.contains(&gone), "{gone}");
    }
    for kept in [
        "Machines",
        "Appearance and skin",
        "Instructions",
        "Plugins",
        "GitHub token",
        "Launch at login",
    ] {
        assert!(settings.contains(&kept), "{kept}");
    }
}

#[test]
fn settings_carry_their_section() {
    let index = build_index(&PaletteLists::default(), &[]);
    let key = index
        .settings
        .iter()
        .find(|e| e.title == "GitHub token")
        .expect("the GitHub token is a setting");
    assert_eq!(key.id, "creds");
    assert_eq!(key.detail, "Settings · Credentials");
    assert_eq!(key.keys, ["github token"]);
}

#[test]
fn a_project_on_two_machines_is_listed_once() {
    let lists = PaletteLists {
        projects: Some(vec![
            imported("acme/widget", "build-box"),
            imported("acme/widget", "mac-box"),
            imported("acme/relay", "build-box"),
        ]),
        ..PaletteLists::default()
    };
    let projects = build_index(&lists, &[]).projects;
    assert_eq!(titles(&projects), ["acme/widget", "acme/relay"]);
    assert_eq!(projects[0].detail, "build-box");
    assert_eq!(projects[0].keys, ["acme/widget"]);
}

#[test]
fn branches_with_work_are_found() {
    let lists = PaletteLists {
        branches: Some(vec![BranchRow {
            project: "acme/widget".to_owned(),
            name: "fix-login".to_owned(),
            ahead: 3,
            subject: "Retry the token once".to_owned(),
        }]),
        ..PaletteLists::default()
    };
    let code = build_index(&lists, &[]).code;
    assert_eq!(code[0].kind, HitKind::Branch);
    assert_eq!(code[0].id, "acme/widget fix-login");
    assert_eq!(code[0].detail, "Branch · 3 ahead · Retry the token once");
}

#[test]
fn a_folder_only_coding_agents_worked_in_is_a_project() {
    let sessions = vec![
        agent_session("1", Some("Desktop UI"), "/work/widget", "build-box", 5),
        agent_session("2", None, "/work/widget", "mac-box", 9),
    ];
    let lists = PaletteLists {
        projects: Some(vec![imported("acme/relay", "build-box")]),
        ..PaletteLists::default()
    };
    let projects = build_index(&lists, &sessions).projects;
    assert_eq!(titles(&projects), ["acme/relay", "widget"]);
    assert_eq!(projects[1].id, "widget");
    assert_eq!(
        projects[1].detail,
        "Not imported · 2 sessions · build-box, mac-box"
    );
}

#[test]
fn projects_are_listed_once_across_lists() {
    let sessions = vec![
        // Inside acme/relay's folder on its machine.
        agent_session("1", None, "/srv/project/sub", "build-box", 1),
        // A checkout named like acme/widget.
        agent_session("2", None, "/home/user/Widget", "mac-box", 2),
    ];
    let lists = PaletteLists {
        projects: Some(vec![
            imported("acme/relay", "build-box"),
            imported("acme/widget", "build-box"),
        ]),
        ..PaletteLists::default()
    };
    let index = build_index(&lists, &sessions);
    assert_eq!(titles(&index.projects), ["acme/relay", "acme/widget"]);
    // The newest session, in the checkout, names the project it lists under.
    assert_eq!(index.sessions[0].project.as_deref(), Some("acme/widget"));
}

#[test]
fn coding_agents_are_found_by_title_project_harness_and_machine() {
    let sessions = vec![agent_session(
        "s1",
        Some("Cross-machine desktop UI"),
        "/srv/project/widget",
        "build-box",
        1,
    )];
    let lists = PaletteLists::default();
    let sessions = build_index(&lists, &sessions).sessions;
    assert_eq!(sessions[0].title, "Cross-machine desktop UI");
    assert_eq!(sessions[0].detail, "widget");
    assert_eq!(
        sessions[0].keys,
        [
            "cross-machine desktop ui",
            "widget",
            "claude code",
            "claude",
            "build-box"
        ]
    );
    assert!(sessions[0].session.is_some());
}

#[test]
fn coding_agents_list_newest_first() {
    let sessions = vec![
        agent_session("a", Some("Old"), "/srv/a", "build-box", 1),
        agent_session("b", Some("New"), "/srv/b", "build-box", 9),
    ];
    let lists = PaletteLists::default();
    let index = build_index(&lists, &sessions);
    assert_eq!(titles(&index.sessions), ["New", "Old"]);
}

#[test]
fn an_untitled_session_is_named_by_its_project() {
    let sessions = vec![agent_session(
        "s1",
        None,
        "/srv/project/src",
        "build-box",
        1,
    )];
    let lists = PaletteLists {
        projects: Some(vec![imported("acme/widget", "build-box")]),
        ..PaletteLists::default()
    };
    let index = build_index(&lists, &sessions);
    assert_eq!(index.sessions[0].title, "acme/widget");
    assert_eq!(index.sessions[0].detail, "acme/widget");
    assert_eq!(titles(&index.projects), ["acme/widget"]);
}
