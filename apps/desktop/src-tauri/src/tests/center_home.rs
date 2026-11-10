use super::*;
use beep_proto::Approval;
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

fn approval(session: Uuid) -> ChatApproval {
    ChatApproval {
        session_id: session,
        session_title: "Chat".into(),
        pending: PendingApproval {
            run_id: "run".into(),
            query_id: "query".into(),
            approval: Approval {
                app: "Fastmail".into(),
                action: "send email".into(),
                fields: Vec::new(),
            },
        },
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
        researches: Ok(Vec::new()),
        approvals: Ok(Vec::new()),
        chats: Ok(Vec::new()),
        crons: Ok(Vec::new()),
        updates: Ok(Vec::new()),
        projects: Ok(Vec::new()),
    }
}

fn imported(project: &str, machine: &str) -> ImportedProject {
    ImportedProject {
        project: project.into(),
        url: format!("https://github.com/{project}"),
        machine_id: machine.into(),
        machine_name: "box".into(),
        folder: format!("/srv/{project}"),
        cloned: false,
        imported_at: at(9000),
    }
}

#[test]
fn projects_come_from_the_mesh_and_the_server_adds_machines_outside_it() {
    let mesh = vec![imported("org/repo", "m1")];
    let both = with_server(
        mesh.clone(),
        Ok(vec![imported("org/repo", "m1"), imported("org/repo", "m2")]),
    )
    .expect("listed");
    assert_eq!(both.len(), 2);
    assert_eq!(both[1].machine_id, "m2");
    // No server, or one that is down, leaves the mesh's own.
    for failed in [
        DesktopError::NoServer,
        DesktopError::Unlinked,
        DesktopError::BadRequest("the server took too long".into()),
    ] {
        assert_eq!(
            with_server(mesh.clone(), Err(failed)).expect("kept").len(),
            1
        );
    }
    // With nothing in the mesh the failure shows, so Home keeps what it had.
    assert!(with_server(Vec::new(), Err(DesktopError::BadRequest("down".into()))).is_err());
    assert!(
        with_server(Vec::new(), Err(DesktopError::NoServer))
            .expect("none")
            .is_empty()
    );
}

#[test]
fn imported_projects_list_and_an_old_server_hides_them() {
    let listed = view(HomeParts {
        projects: Ok(vec![ImportedProject {
            project: "org/repo".into(),
            url: "https://github.com/org/repo".into(),
            machine_id: "m1".into(),
            machine_name: "build-box".into(),
            folder: "/srv/work/projects/org/repo".into(),
            cloned: false,
            imported_at: at(9000),
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
fn updates_come_with_their_app_and_time() {
    let listed = view(HomeParts {
        updates: Ok(vec![AppUpdate {
            id: 7,
            summary: "The build is ready.".into(),
            app: "gmail".into(),
            app_name: "Gmail".into(),
            at: at(5000),
        }]),
        ..parts()
    });
    let updates = listed.updates.expect("updates were listed");
    assert_eq!(updates.len(), 1);
    assert_eq!((updates[0].id, updates[0].at), (7, 5000));
    assert_eq!(updates[0].app_name, "Gmail");
    let old = view(HomeParts {
        updates: Err(DesktopError::Api(ApiError::bad_request("unknown op 160"))),
        ..parts()
    });
    assert!(old.updates.is_none(), "a server without updates hides them");
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
    assert!(view.coming_up.is_some());
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
    .expect("an old server is not a failure");
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
    let chat = Uuid::new_v4();
    let view = view(HomeParts {
        sessions: Ok(vec![
            asking("build-box", "late", 3000),
            asking("build-box", "early", 1000),
        ]),
        approvals: Ok(vec![approval(chat)]),
        chats: Ok(vec![Session {
            id: chat.to_string(),
            title: "Chat".into(),
            pinned: false,
            updated_at: 2000,
        }]),
        ..parts()
    });
    let order: Vec<String> = view
        .needs_you
        .iter()
        .map(|item| match item {
            NeedsYou::Chat { .. } => "chat".into(),
            NeedsYou::Agent { session, .. } => session.clone(),
        })
        .collect();
    assert_eq!(order, ["early", "chat", "late"]);
}

#[test]
fn approval_of_a_missing_chat_comes_last() {
    let view = view(HomeParts {
        sessions: Ok(vec![asking("build-box", "agent", 5000)]),
        approvals: Ok(vec![approval(Uuid::new_v4())]),
        ..parts()
    });
    assert!(matches!(view.needs_you[0], NeedsYou::Agent { .. }));
    assert!(matches!(view.needs_you[1], NeedsYou::Chat { .. }));
}
