//! The pages that list a user's imported projects and their machines, from the records.

use crate::{
    account::user::User,
    protocol::{ImportedProject, ProjectMachine},
};
use beep_projects::imports::{flow::project, records::ImportRow};
use std::collections::{HashMap, HashSet};

/// Every linked machine as `project`'s page lists it, those with its folder first; each is
/// offline to the server. `None` when not imported.
pub(crate) async fn project_machines(
    user: &User,
    project: &str,
) -> anyhow::Result<Option<Vec<ProjectMachine>>> {
    let rows: Vec<ImportRow> = user
        .imports()
        .await?
        .into_iter()
        .filter(|row| row.project.eq_ignore_ascii_case(project))
        .collect();
    if rows.is_empty() {
        return Ok(None);
    }

    let linked: HashSet<String> = user
        .account_keys()
        .await?
        .into_iter()
        .map(|key| key.endpoint_id)
        .collect();
    let mut machines: Vec<ProjectMachine> = user
        .nodes()
        .await?
        .into_iter()
        .filter(|node| linked.contains(&node.endpoint))
        .map(|node| ProjectMachine {
            folder: rows
                .iter()
                .find(|row| row.endpoint == node.endpoint)
                .map(|row| row.folder.clone()),
            machine_id: node.endpoint,
            machine_name: node.name,
            online: false,
            checking: false,
            checked_at: None,
        })
        .collect();
    machines.sort_by(|a, b| {
        b.folder
            .is_some()
            .cmp(&a.folder.is_some())
            .then_with(|| a.machine_name.cmp(&b.machine_name))
    });
    Ok(Some(machines))
}

/// The user's imported projects on machines still theirs.
pub(crate) async fn imported(user: &User) -> anyhow::Result<Vec<ImportedProject>> {
    let names: HashMap<String, String> = user
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.endpoint, node.name))
        .collect();
    Ok(user
        .imports()
        .await?
        .into_iter()
        .filter_map(|row| {
            let name = names.get(&row.endpoint)?.clone();
            Some(project(row, name, false))
        })
        .collect())
}
