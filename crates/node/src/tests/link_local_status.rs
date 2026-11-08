use super::*;
use iroh::SecretKey;

fn project(name: &str, state: State, held: bool, missing: bool) -> Project {
    Project {
        status: ProjectStatus {
            name: name.to_owned(),
            folder: format!("/srv/{name}"),
            state,
            error: (state == State::Failed).then(|| "no such disk".to_owned()),
            ..ProjectStatus::default()
        },
        held,
        missing,
    }
}

#[test]
fn says_each_device_and_how_each_project_syncs() {
    let me = SecretKey::generate().public();
    let other = SecretKey::generate().public();
    let desk = SecretKey::generate().public();
    let peer = |id, name: &str, kind, online, last_seen| Peer {
        id,
        name: name.to_owned(),
        kind,
        online,
        last_seen,
    };
    let status = LocalStatus {
        version: VERSION.to_owned(),
        started: 1_000,
        name: "build-box".to_owned(),
        id: me,
        mesh: Some(desk),
        devices: vec![
            peer(desk, "laptop", DeviceKind::Desktop, true, None),
            peer(
                other,
                "gpu-box",
                DeviceKind::Node,
                false,
                Some(1_000 + 3_000),
            ),
            peer(me, "build-box", DeviceKind::Node, true, None),
        ],
        projects: vec![
            project("api", State::Idle, false, false),
            project("notes", State::Failed, true, false),
            project("old", State::Failed, false, true),
            project("docs", State::Failed, false, false),
        ],
        agents: vec![
            Agent {
                name: "claude".to_owned(),
                state: AgentState::Failed {
                    reason: "needs Node.js: curl failed".to_owned(),
                },
            },
            Agent {
                name: "codex".to_owned(),
                state: AgentState::Installing,
            },
        ],
    };
    let text = render(&status, 1_000 + 3_720);
    assert!(
        text.contains("claude    not ready, needs Node.js: curl failed"),
        "{text}"
    );
    assert!(
        text.contains("codex     found, installing its adapter"),
        "{text}"
    );
    assert!(text.contains("running for 1h 2m"), "{text}");
    assert!(text.contains("2 of 3 devices online"), "{text}");
    assert!(text.contains("offline, last seen 12m ago"), "{text}");
    assert!(text.contains("this machine"), "{text}");
    for (name, state) in [
        ("api", "in sync"),
        ("notes", "waiting for Syncthing"),
        ("old", "folder missing"),
        ("docs", "not syncing: no such disk"),
    ] {
        let row = text
            .lines()
            .find(|line| line.trim_start().starts_with(name))
            .unwrap_or_else(|| panic!("no row for {name}: {text}"));
        assert!(row.contains(state), "{row}");
    }
}

#[test]
fn spans_read_as_people_say_them() {
    assert_eq!(span(45), "45s");
    assert_eq!(span(720), "12m");
    assert_eq!(span(3 * 3600 + 240), "3h 4m");
    assert_eq!(span(2 * 86_400 + 5 * 3600), "2d 5h");
}
