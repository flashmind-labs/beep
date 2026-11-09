//! Checkouts the user's coding agents worked in, one row per repository, excluding imported ones.

use super::{
    records::ImportRow,
    repo::{RepoRef, remote_key},
};
use crate::model::ForgejoUrl;
use beep_proto::{FoundAgent, FoundPlace, FoundProject, node::agent::SessionFolder};
use std::{
    cmp::Reverse,
    collections::HashMap,
    time::{Duration, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct MachineFound {
    pub machine_id: String,
    pub machine_name: String,
    pub folders: Vec<SessionFolder>,
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
pub fn group(
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
#[path = "../tests/imports_found.rs"]
mod tests;
