use super::*;
use beep_proto::node::agent::AgentSessions;
use chrono::Utc;
use std::time::SystemTime;

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn folder(path: &str, origin: &str, agents: &[(&str, u32)], last: u64) -> SessionFolder {
    SessionFolder {
        path: path.into(),
        origin: origin.into(),
        agents: agents
            .iter()
            .map(|(agent, sessions)| AgentSessions {
                agent: (*agent).into(),
                sessions: *sessions,
            })
            .collect(),
        last_activity: last,
    }
}

fn machine(id: &str, folders: Vec<SessionFolder>) -> MachineFound {
    MachineFound {
        machine_id: id.into(),
        machine_name: format!("{id}-box"),
        folders,
    }
}

fn imported(project: &str, endpoint: &str, folder: &str) -> ImportRow {
    ImportRow {
        project: project.into(),
        endpoint: endpoint.into(),
        folder: folder.into(),
        url: format!("https://github.com/{project}"),
        imported_at: Utc::now(),
        fork: None,
    }
}

#[test]
fn one_row_per_repository_across_machines() {
    let found = vec![
        machine(
            "a",
            vec![
                folder(
                    "/home/user/proj",
                    "git@github.com:org/proj.git",
                    &[("claude", 4)],
                    50,
                ),
                folder("/home/user/loose", "", &[("pi", 2)], 90),
            ],
        ),
        machine(
            "b",
            vec![
                folder(
                    "/srv/proj",
                    "https://github.com/org/proj",
                    &[("codex", 2), ("claude", 1)],
                    70,
                ),
                folder(
                    "/srv/other",
                    "https://git.example.com/team/other.git",
                    &[("flsh", 1)],
                    10,
                ),
            ],
        ),
    ];
    let rows = group(found, &[], &[]);
    assert_eq!(rows.len(), 2, "{rows:?}");
    let proj = &rows[0];
    assert_eq!(proj.project, "org/proj");
    assert_eq!(proj.last_activity, at(70));
    let places: Vec<(&str, &str)> = proj
        .places
        .iter()
        .map(|place| (place.machine_id.as_str(), place.folder.as_str()))
        .collect();
    assert_eq!(places, [("b", "/srv/proj"), ("a", "/home/user/proj")]);
    assert_eq!(
        proj.agents,
        [
            FoundAgent {
                agent: "claude".into(),
                sessions: 5
            },
            FoundAgent {
                agent: "codex".into(),
                sessions: 2
            },
        ]
    );
    assert_eq!(rows[1].project, "team/other");
    assert_eq!(rows[1].remote, "https://git.example.com/team/other.git");
}

#[test]
fn imported_repositories_and_folders_are_left_out() {
    let found = vec![machine(
        "a",
        vec![
            folder(
                "/home/user/proj",
                "git@github.com:org/proj.git",
                &[("claude", 1)],
                5,
            ),
            folder(
                "/home/user/kept",
                "git@github.com:org/kept.git",
                &[("claude", 1)],
                4,
            ),
            folder(
                "/home/user/fork",
                "git@github.com:me/fork.git",
                &[("claude", 1)],
                3,
            ),
        ],
    )];
    let mut moved = imported("me/renamed", "a", "/home/user/fork");
    moved.url = "https://github.com/me/renamed".into();
    let rows = group(found, &[imported("org/proj", "b", "/srv/proj"), moved], &[]);
    let names: Vec<&str> = rows.iter().map(|row| row.project.as_str()).collect();
    assert_eq!(names, ["org/kept"]);
}
