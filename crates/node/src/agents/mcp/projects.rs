//! Project discovery and checkout selection from replicated mesh records.

use super::machines::{find, machines};
use crate::agents::reply::{fail, json_of, parse};
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

pub(super) fn call(mesh: Option<&Mesh>, args: &str) -> Reply {
    list(mesh, args).unwrap_or_else(fail)
}

fn list(mesh: Option<&Mesh>, args: &str) -> Result<Reply, String> {
    let args: Args = parse(args)?;
    let mesh = mesh.ok_or("This machine is in no mesh, so it has no registered projects.")?;
    let machines = machines(mesh);
    let machine = args
        .machine
        .as_deref()
        .map(|name| {
            find(mesh, name).map(|m| m.id).ok_or_else(|| {
                format!("No machine called {name}. Use list_machines to find its name.")
            })
        })
        .transpose()?;
    let project = args
        .project
        .as_deref()
        .map(|name| project_name(mesh, name))
        .transpose()?;
    let mut projects = Vec::new();

    for (name, url) in names(mesh) {
        if project.as_ref().is_some_and(|wanted| wanted != &name) {
            continue;
        }

        let checkouts: Vec<Checkout> = machines
            .iter()
            .filter(|host| machine.is_none_or(|wanted| wanted == host.id))
            .filter_map(|host| {
                project_folder(mesh, &name, &host.id).map(|cwd| Checkout {
                    machine: host.name.clone(),
                    cwd,
                })
            })
            .collect();

        if machine.is_none() || !checkouts.is_empty() {
            let display_name = project_record(mesh, &name).and_then(|record| record.display_name);
            projects.push(Project {
                name,
                display_name,
                url,
                checkouts,
            });
        }
    }

    Ok(json_of(&projects))
}

/// Refuses a session of `agent` in `cwd` when the project whose checkout here holds it limits
/// the harnesses that run in it.
pub(crate) fn may_run(mesh: &Mesh, cwd: &Path, agent: &str) -> Result<(), String> {
    let Some(name) = of_folder(mesh, &cwd.to_string_lossy(), mesh.me()) else {
        return Ok(());
    };
    let Some(record) = project_record(mesh, &name) else {
        return Ok(());
    };
    let Some(allowed) = record.harnesses else {
        return Ok(());
    };

    if allowed.iter().any(|harness| harness.as_ref() == agent) {
        return Ok(());
    }

    let project = record.display_name.unwrap_or(name);
    let allowed: Vec<&str> = allowed.iter().map(AsRef::as_ref).collect();

    Err(match allowed.as_slice() {
        [] => format!("No agent may run in {project}, as set in its Harnesses."),
        _ => format!(
            "{agent} may not run in {project}: it allows only {}, as set in its Harnesses.",
            allowed.join(", ")
        ),
    })
}

pub(super) fn project_name(mesh: &Mesh, name: &str) -> Result<String, String> {
    let trimmed = name.trim();

    names(mesh)
        .into_keys()
        .find(|known| {
            known.eq_ignore_ascii_case(trimmed)
                || project_record(mesh, known)
                    .and_then(|record| record.display_name)
                    .is_some_and(|label| label.eq_ignore_ascii_case(trimmed))
        })
        .ok_or_else(|| format!("No project called {name}. Use list_projects to find its name."))
}

pub(super) fn folder(mesh: &Mesh, name: &str, machine: EndpointId) -> Result<String, String> {
    let name = project_name(mesh, name)?;

    project_folder(mesh, &name, &machine)
        .filter(|folder| !folder.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "Project {name} has no checkout on that machine. Use list_projects to find one."
            )
        })
}

/// The project whose checkout on `machine` holds `cwd`, the innermost when
/// checkouts nest.
pub(crate) fn of_folder(mesh: &Mesh, cwd: &str, machine: EndpointId) -> Option<String> {
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

pub(super) fn sync_specs(mesh: &Mesh) -> Vec<(String, SyncSpec)> {
    mesh.scan("sync/")
        .into_iter()
        .filter_map(|(key, bytes)| match key {
            Key::Sync(name) => Some((name, SyncSpec::from_bytes(&bytes).ok()?)),
            _ => None,
        })
        .collect()
}

fn project_record(mesh: &Mesh, name: &str) -> Option<ProjectRecord> {
    let bytes = mesh.get(&Key::Project(name.to_owned()))?;

    ProjectRecord::from_bytes(&bytes).ok()
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

    for (name, _) in sync_specs(mesh) {
        names.entry(name).or_insert(None);
    }

    names
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_projects.rs"]
mod tests;
