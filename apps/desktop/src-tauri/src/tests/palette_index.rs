use super::super::lists::{BranchRow, PrRow};
use super::*;
use beep_proto::{AgentStatus, ImportedProject, MachineSession, ProjectAgent};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

fn imported(project: &str, machine: &str) -> ImportedProject {
    ImportedProject {
        project: project.to_owned(),
        url: format!("https://example.com/{project}"),
        machine_id: machine.to_owned(),
        machine_name: machine.to_owned(),
        folder: "/srv/project".to_owned(),
        cloned: false,
        imported_at: SystemTime::UNIX_EPOCH,
    }
}

fn pr(repo: &str, number: u64, title: &str) -> PrRow {
    PrRow {
        repo: repo.to_owned(),
        number,
        title: title.to_owned(),
        url: format!("https://example.com/{repo}/pull/{number}"),
        detail: "PR · yours · example.com".to_owned(),
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
        machine_id: machine.to_owned(),
        machine_name: machine.to_owned(),
        id: id.to_owned(),
        agent: "claude".to_owned(),
        title: title.map(str::to_owned),
        cwd: cwd.to_owned(),
        status: AgentStatus::Working {},
        doing: None,
        started_at: SystemTime::UNIX_EPOCH,
        last_activity: SystemTime::UNIX_EPOCH + Duration::from_secs(at),
        external: false,
        question: None,
    }
}

fn board(name: &str, chat: u128) -> ProjectAgent {
    ProjectAgent {
        project_id: Uuid::from_u128(chat + 1000),
        name: name.to_owned(),
        chat_id: Uuid::from_u128(chat),
        open_tasks: 0,
        open_workers: 0,
    }
}

fn titles(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.title.as_str()).collect()
}

#[test]
fn with_no_subscription_the_assistants_pages_are_not_found() {
    let lists = PaletteLists {
        without_subscription: true,
        ..PaletteLists::default()
    };
    let index = build_index(&lists);
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
    for kept in ["Home", "Settings", "Coding agents"] {
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
    for kept in ["Machines", "GitHub token", "Launch at login"] {
        assert!(settings.contains(&kept), "{kept}");
    }
    // With one, all of them are there.
    let all = build_index(&PaletteLists::default());
    assert!(titles(&all.actions).contains(&"Memories"));
}

#[test]
fn settings_carry_their_section() {
    let index = build_index(&PaletteLists::default());
    let key = index
        .settings
        .iter()
        .find(|e| e.title == "GitHub token")
        .expect("the GitHub token is a setting");
    assert_eq!(key.id, "creds");
    assert_eq!(key.detail, "Settings · Credentials");
    assert_eq!(key.keys, ["github token"]);
    let thinking = index
        .settings
        .iter()
        .filter(|e| e.keys[0].contains("thinking"));
    assert!(thinking.clone().count() > 1);
    assert!(thinking.clone().all(|e| e.id == "models"));
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
    let projects = build_index(&lists).projects;
    assert_eq!(titles(&projects), ["acme/widget", "acme/relay"]);
    assert_eq!(projects[0].detail, "build-box");
    assert_eq!(projects[0].keys, ["acme/widget"]);
}

#[test]
fn pull_requests_are_found_by_title_repo_or_number_then_branches() {
    let lists = PaletteLists {
        prs: Some(vec![pr("Acme/Widget", 42, "Fix the login loop")]),
        branches: Some(vec![BranchRow {
            project: "acme/widget".to_owned(),
            name: "fix-login".to_owned(),
            ahead: 3,
            subject: "Retry the token once".to_owned(),
        }]),
        ..PaletteLists::default()
    };
    let code = build_index(&lists).code;
    assert_eq!(code[0].id, "https://example.com/Acme/Widget/pull/42");
    assert_eq!(code[0].title, "Acme/Widget · PR #42 Fix the login loop");
    assert_eq!(code[0].keys, ["fix the login loop", "acme/widget", "#42"]);
    assert_eq!(code[1].kind, HitKind::Branch);
    assert_eq!(code[1].id, "acme/widget fix-login");
    assert_eq!(code[1].detail, "Branch · 3 ahead · Retry the token once");
}

#[test]
fn a_folder_only_coding_agents_worked_in_is_a_project() {
    let lists = PaletteLists {
        projects: Some(vec![imported("acme/relay", "build-box")]),
        sessions: Some(vec![
            agent_session("1", Some("Desktop UI"), "/work/widget", "build-box", 5),
            agent_session("2", None, "/work/widget", "mac-box", 9),
        ]),
        ..PaletteLists::default()
    };
    let projects = build_index(&lists).projects;
    assert_eq!(titles(&projects), ["acme/relay", "widget"]);
    assert_eq!(projects[1].id, "widget");
    assert_eq!(
        projects[1].detail,
        "Not imported · 2 sessions · build-box, mac-box"
    );
}

#[test]
fn a_board_project_that_is_not_imported_opens_its_chat() {
    let lists = PaletteLists {
        projects: Some(vec![imported("acme/relay", "build-box")]),
        boards: Some(vec![board("acme/relay", 1), board("acme/widget", 2)]),
        ..PaletteLists::default()
    };
    let projects = build_index(&lists).projects;
    assert_eq!(titles(&projects), ["acme/relay", "acme/widget"]);
    assert_eq!(projects[1].id, Uuid::from_u128(2).to_string());
}

#[test]
fn projects_are_listed_once_across_lists() {
    let lists = PaletteLists {
        projects: Some(vec![imported("acme/relay", "build-box")]),
        boards: Some(vec![board("acme/relay", 1), board("acme/widget", 2)]),
        sessions: Some(vec![
            // Inside acme/relay's folder on its machine.
            agent_session("1", None, "/srv/project/sub", "build-box", 1),
            // A checkout named like the board's acme/widget.
            agent_session("2", None, "/home/user/Widget", "mac-box", 2),
        ]),
        ..PaletteLists::default()
    };
    let projects = build_index(&lists).projects;
    assert_eq!(titles(&projects), ["acme/relay", "acme/widget"]);
}

#[test]
fn coding_agents_are_found_by_title_project_harness_and_machine() {
    let lists = PaletteLists {
        sessions: Some(vec![agent_session(
            "s1",
            Some("Cross-machine desktop UI"),
            "/srv/project/widget",
            "build-box",
            1,
        )]),
        ..PaletteLists::default()
    };
    let sessions = build_index(&lists).sessions;
    assert_eq!(sessions[0].title, "Cross-machine desktop UI");
    assert_eq!(sessions[0].detail, "Claude Code · widget · build-box");
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
fn coding_agents_and_chats_list_newest_first() {
    let session = |id: &str, updated_at| Session {
        id: id.to_owned(),
        title: id.to_owned(),
        pinned: id == "old",
        updated_at,
    };
    let lists = PaletteLists {
        sessions: Some(vec![
            agent_session("a", Some("Old"), "/srv/a", "build-box", 1),
            agent_session("b", Some("New"), "/srv/b", "build-box", 9),
        ]),
        chats: Some(vec![session("old", 1), session("new", 5)]),
        ..PaletteLists::default()
    };
    let index = build_index(&lists);
    assert_eq!(titles(&index.sessions), ["New", "Old"]);
    assert_eq!(titles(&index.chats), ["new", "old"]);
}

#[test]
fn an_untitled_session_is_named_by_its_project() {
    let lists = PaletteLists {
        projects: Some(vec![imported("acme/widget", "build-box")]),
        sessions: Some(vec![agent_session(
            "s1",
            None,
            "/srv/project/src",
            "build-box",
            1,
        )]),
        ..PaletteLists::default()
    };
    let index = build_index(&lists);
    assert_eq!(index.sessions[0].title, "acme/widget");
    assert_eq!(
        index.sessions[0].detail,
        "Claude Code · acme/widget · build-box"
    );
    assert_eq!(titles(&index.projects), ["acme/widget"]);
}
