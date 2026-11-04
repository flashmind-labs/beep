//! Checkouts the user's coding agents worked in, one row per repository, excluding imported ones.

use super::repo::{RepoRef, remote_key};
use crate::{
    account::credentials::ForgejoUrl,
    protocol::{FoundAgent, FoundPlace, FoundProject, node::agent::SessionFolder},
    store::ImportRow,
};
use std::{
    cmp::Reverse,
    collections::HashMap,
    time::{Duration, UNIX_EPOCH},
};

/// What one machine found.
pub(crate) struct MachineFound {
    pub(crate) machine_id: String,
    pub(crate) machine_name: String,
    pub(crate) folders: Vec<SessionFolder>,
}

/// One repository as it is being gathered.
struct Row {
    remote: String,
    /// The folder per machine, by machine id, with its last activity.
    places: Vec<(FoundPlace, u64)>,
    agents: HashMap<String, u32>,
    last_activity: u64,
}

/// One row per found repository, newest first, skipping remoteless and already imported checkouts.
pub(crate) fn group(
    found: Vec<MachineFound>,
    imported: &[ImportRow],
    servers: &[ForgejoUrl],
) -> Vec<FoundProject> {
    let imported_repos: Vec<RepoRef> = imported
        .iter()
        .filter_map(|row| RepoRef::parse(&row.url, servers).ok())
        .collect();
    let imported_keys: Vec<String> = imported
        .iter()
        .filter_map(|row| remote_key(&row.url))
        .collect();
    let is_imported = |remote: &str, key: &str| {
        imported_keys.iter().any(|known| known == key)
            || imported_repos
                .iter()
                .any(|repo| repo.matches_remote(remote))
    };
    let mut rows: HashMap<String, Row> = HashMap::new();
    for machine in found {
        for folder in machine.folders {
            let Some(key) = remote_key(&folder.origin) else {
                continue;
            };
            let here = imported
                .iter()
                .any(|row| row.endpoint == machine.machine_id && row.folder == folder.path);
            if here || is_imported(&folder.origin, &key) {
                continue;
            }
            let row = rows.entry(key).or_insert_with(|| Row {
                remote: folder.origin.clone(),
                places: Vec::new(),
                agents: HashMap::new(),
                last_activity: 0,
            });
            for counted in &folder.agents {
                *row.agents.entry(counted.agent.clone()).or_default() += counted.sessions;
            }
            row.last_activity = row.last_activity.max(folder.last_activity);
            let place = FoundPlace {
                machine_id: machine.machine_id.clone(),
                machine_name: machine.machine_name.clone(),
                folder: folder.path,
            };
            // One folder per machine: the busiest lately.
            match row
                .places
                .iter_mut()
                .find(|(known, _)| known.machine_id == place.machine_id)
            {
                Some(known) if known.1 < folder.last_activity => {
                    *known = (place, folder.last_activity)
                }
                Some(_) => {}
                None => row.places.push((place, folder.last_activity)),
            }
        }
    }
    let mut out: Vec<FoundProject> = rows
        .into_iter()
        .map(|(key, row)| project(&key, row, servers))
        .collect();
    out.sort_by(|a, b| {
        b.last_activity
            .cmp(&a.last_activity)
            .then_with(|| a.project.cmp(&b.project))
    });
    out
}

fn project(key: &str, mut row: Row, servers: &[ForgejoUrl]) -> FoundProject {
    let name = match RepoRef::parse(&row.remote, servers) {
        Ok(repo) => repo.project(),
        // A host without a token: its last two parts read as `org/repo`.
        Err(_) => {
            let parts: Vec<&str> = key.rsplitn(3, '/').collect();
            match parts[..] {
                [repo, org, _] => format!("{org}/{repo}"),
                _ => row.remote.clone(),
            }
        }
    };
    row.places
        .sort_by_key(|(place, at)| (Reverse(*at), place.machine_name.clone()));
    let mut agents: Vec<FoundAgent> = row
        .agents
        .into_iter()
        .map(|(agent, sessions)| FoundAgent { agent, sessions })
        .collect();
    agents.sort_by(|a, b| {
        b.sessions
            .cmp(&a.sessions)
            .then_with(|| a.agent.cmp(&b.agent))
    });
    FoundProject {
        project: name,
        remote: row.remote,
        places: row.places.into_iter().map(|(place, _)| place).collect(),
        agents,
        last_activity: UNIX_EPOCH + Duration::from_secs(row.last_activity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::agent::AgentSessions;
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
}
