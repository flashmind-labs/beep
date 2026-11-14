//! Project discovery and checkout selection from replicated mesh records.

use super::{
    machines::{find, machines},
    reply::{fail, json_of, parse},
};
use crate::agents::moving::project_folder;
use beep_mesh::{Key, Mesh};
use beep_proto::node::{
    mesh_state::{Place, ProjectRecord, StateValue, SyncSpec},
    tools::Reply,
};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Serialize)]
struct Project {
    name: String,
    display_name: Option<String>,
    url: Option<String>,
    checkouts: Vec<Checkout>,
}

#[derive(Serialize)]
struct Checkout {
    machine: String,
    cwd: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    project: Option<String>,
    machine: Option<String>,
}

fn names(mesh: &Mesh) -> BTreeMap<String, Option<String>> {
    let mut names = BTreeMap::new();

    for (key, bytes) in mesh.scan("project/") {
        let Key::Project(name) = key else { continue };

        if let Some((project, machine)) = name.rsplit_once('/')
            && machine.parse::<EndpointId>().is_ok()
            && Place::from_bytes(&bytes).is_ok()
        {
            names.entry(project.to_owned()).or_insert(None);
        } else if let Ok(record) = ProjectRecord::from_bytes(&bytes) {
            names.insert(name, Some(record.url));
        }
    }

    for (key, bytes) in mesh.scan("sync/") {
        if let Key::Sync(name) = key
            && SyncSpec::from_bytes(&bytes).is_ok()
        {
            names.entry(name).or_insert(None);
        }
    }

    names
}

pub(super) fn project_name(mesh: &Mesh, name: &str) -> Result<String, String> {
    names(mesh)
        .into_keys()
        .find(|known| {
            known.eq_ignore_ascii_case(name.trim())
                || mesh
                    .get(&Key::Project(known.clone()))
                    .and_then(|bytes| ProjectRecord::from_bytes(&bytes).ok())
                    .and_then(|record| record.display_name)
                    .is_some_and(|label| label.eq_ignore_ascii_case(name.trim()))
        })
        .ok_or_else(|| format!("No project called {name}. Use machines projects to find its name."))
}

pub(super) fn folder(mesh: &Mesh, name: &str, machine: EndpointId) -> Result<String, String> {
    let name = project_name(mesh, name)?;
    project_folder(mesh, &name, &machine)
        .filter(|folder| !folder.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "Project {name} has no checkout on that machine. Use machines projects to find one."
            )
        })
}

/// The project whose checkout on `machine` holds `cwd`, the innermost when
/// checkouts nest.
pub(super) fn of_folder(mesh: &Mesh, cwd: &str, machine: EndpointId) -> Option<String> {
    names(mesh)
        .into_keys()
        .filter_map(|name| {
            let folder = project_folder(mesh, &name, &machine)?;
            let inside = !folder.trim().is_empty() && Path::new(cwd).starts_with(&folder);
            inside.then_some((folder.len(), name))
        })
        .max()
        .map(|(_, name)| name)
}

pub(super) fn call(mesh: Option<&Mesh>, args: &str) -> Reply {
    let args: Args = match parse(args) {
        Ok(args) => args,
        Err(e) => return fail(e),
    };
    let Some(mesh) = mesh else {
        return fail("This machine is in no mesh, so it has no registered projects.");
    };
    let machines = machines(mesh);
    let machine = match args.machine {
        Some(name) => match find(mesh, &name) {
            Some(machine) => Some(machine.id),
            None => {
                return fail(format!(
                    "No machine called {name}. Use machines list to find its name."
                ));
            }
        },
        None => None,
    };
    let project = match args.project {
        Some(name) => match project_name(mesh, &name) {
            Ok(name) => Some(name),
            Err(e) => return fail(e),
        },
        None => None,
    };
    let mut projects = Vec::new();

    for (name, url) in names(mesh) {
        if project.as_ref().is_some_and(|wanted| wanted != &name) {
            continue;
        }

        let mut checkouts = Vec::new();

        for host in &machines {
            if machine.is_some_and(|wanted| wanted != host.id) {
                continue;
            }

            if let Some(cwd) = project_folder(mesh, &name, &host.id) {
                checkouts.push(Checkout {
                    machine: host.name.clone(),
                    cwd,
                });
            }
        }

        if machine.is_none() || !checkouts.is_empty() {
            let display_name = mesh
                .get(&Key::Project(name.clone()))
                .and_then(|bytes| ProjectRecord::from_bytes(&bytes).ok())
                .and_then(|record| record.display_name);
            projects.push(Project {
                name,
                display_name,
                url,
                checkouts,
            });
        }
    }

    json_of(&projects)
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_projects.rs"]
mod tests;
