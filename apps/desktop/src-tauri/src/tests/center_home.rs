use super::*;
use std::time::{Duration, SystemTime};

fn at(ms: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
}

fn machine(id: &str, online: bool) -> Machine {
    Machine {
        id: id.into(),
        name: id.into(),
        online,
        seen_at: at(0),
        version: "0.1.0".into(),
        env: None,
    }
}

fn agent(machine: &str, id: &str, last_activity: u64) -> MachineSession {
    MachineSession {
        reply: None,
        changes: None,
        machine_id: machine.into(),
        machine_name: machine.into(),
        id: id.into(),
        agent: "claude".into(),
        title: None,
        cwd: "/srv/project".into(),
        status: AgentStatus::Working {},
        doing: None,
        started_at: at(0),
        last_activity: at(last_activity),
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

fn asking(machine: &str, id: &str, last_activity: u64) -> MachineSession {
    MachineSession {
        question: Some(AgentQuestion {
            id: 1,
            text: "Run cargo test".into(),
            detail: None,
            options: Vec::new(),
        }),
        ..agent(machine, id, last_activity)
    }
}

fn view(parts: HomeParts) -> HomeView {
    home_view(parts).expect("home built")
}

fn parts() -> HomeParts {
    HomeParts {
        machines: Ok(Vec::new()),
        details: Ok(Vec::new()),
        stats: Ok(Vec::new()),
        sessions: Ok(Vec::new()),
        projects: Ok(Vec::new()),
    }
}

#[allow(dead_code)]
#[test]
fn imported_projects_list_and_an_old_machine_hides_them() {
    let listed = view(HomeParts {
        projects: Ok(vec![ImportedProject {
            project: "org/repo".into(),
            url: "https://github.com/org/repo".into(),
            machine_id: "m1".into(),
            machine_name: "build-box".into(),
            folder: "/srv/work/projects/org/repo".into(),
            cloned: false,
            imported_at: at(9000),
            display_name: None,
        }]),
        ..parts()
    });
    let projects = listed.projects.expect("projects were listed");
    assert_eq!(projects.len(), 1);
    let old = view(HomeParts {
        projects: Err(DesktopError::Api(ApiError::bad_request("unknown op 167"))),
        ..parts()
    });
    assert!(old.projects.is_none());
}

#[test]
fn machines_count_their_agents() {
    let view = view(HomeParts {
        machines: Ok(vec![machine("build-box", true), machine("mac-box", true)]),
        sessions: Ok(vec![agent("build-box", "a", 1), agent("build-box", "b", 2)]),
        ..parts()
    });
    let cards = view.machines.expect("machines were listed");
    let build = cards.iter().find(|c| c.id == "build-box").expect("card");
    let mac = cards.iter().find(|c| c.id == "mac-box").expect("card");
    assert_eq!((build.agents, mac.agents), (2, 0));
}

#[test]
fn machines_carry_their_stats() {
    let listed = view(HomeParts {
        machines: Ok(vec![machine("build-box", true), machine("mac-box", true)]),
        stats: Ok(vec![MachineStats {
            id: "build-box".into(),
            sampled_at: at(9000),
            disks: vec![MachineDisk {
                path: "/srv/project".into(),
                free_bytes: 212_000_000_000,
                total_bytes: 500_000_000_000,
            }],
            memory_used_mb: 9300,
            memory_total_mb: 32768,
            load: Some(1.5),
        }]),
        ..parts()
    });
    let cards = listed.machines.expect("machines were listed");
    let build = cards.iter().find(|c| c.id == "build-box").expect("card");
    let stats = build.stats.as_ref().expect("stats");
    assert_eq!((stats.sampled_at, stats.memory_used_mb), (9000, 9300));
    assert!(cards.iter().any(|c| c.id == "mac-box" && c.stats.is_none()));
    let old = view(HomeParts {
        machines: Ok(vec![machine("build-box", true)]),
        stats: Err(DesktopError::Api(ApiError::bad_request("unknown op 165"))),
        ..parts()
    });
    assert!(
        old.machines.expect("machines were listed")[0]
            .stats
            .is_none()
    );
}

#[test]
fn unsupported_section_is_none() {
    let view = view(HomeParts {
        machines: Ok(vec![machine("build-box", true)]),
        sessions: Err(DesktopError::Api(ApiError::bad_request("unknown op 131"))),
        ..parts()
    });
    assert!(view.running.is_none());
    assert!(view.machines.is_some());
}

#[test]
fn machine_list_failure_fails_home() {
    let failed = home_view(HomeParts {
        machines: Err(DesktopError::Other(anyhow::anyhow!("link is down"))),
        ..parts()
    });
    assert!(failed.is_err());
    let unsupported = home_view(HomeParts {
        machines: Err(DesktopError::Api(ApiError::bad_request("unknown op 130"))),
        ..parts()
    })
    .expect("an old machine is not a failure");
    assert!(unsupported.machines.is_none());
}

#[test]
fn machines_sort_online_then_by_agents() {
    let view = view(HomeParts {
        machines: Ok(vec![
            machine("off-busy", false),
            machine("on-idle", true),
            machine("on-busy", true),
            machine("off-idle", false),
        ]),
        sessions: Ok(vec![
            agent("off-busy", "a", 1),
            agent("off-busy", "b", 2),
            agent("on-busy", "c", 3),
        ]),
        ..parts()
    });
    let order: Vec<String> = view
        .machines
        .expect("machines were listed")
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(order, ["on-busy", "on-idle", "off-busy", "off-idle"]);
}

#[test]
fn needs_you_puts_oldest_first() {
    let view = view(HomeParts {
        sessions: Ok(vec![
            asking("build-box", "late", 3000),
            asking("build-box", "early", 1000),
        ]),
        ..parts()
    });
    let order: Vec<SessionId> = view
        .needs_you
        .iter()
        .map(|NeedsYou::Agent { session, .. }| session.clone())
        .collect();
    assert_eq!(order, ["early", "late"]);
}
