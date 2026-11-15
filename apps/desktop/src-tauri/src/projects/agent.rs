//! The `projects` tool of the Beep MCP server, as this desktop answers it for
//! the home assistant: the Git hosts, making and cloning projects, and making
//! projects of the checkouts the machines have. Each action does what the
//! Add a project and New project sheets do.

use super::{
    create::{CreatedRow, ForgeRow},
    import::FoundRow,
};
use crate::{
    fleet::{Fleet, files as direct, projects::desk},
    machines::list::MachineLists,
};
use beep_fleet::{Desk, ImportJob, Session};
use beep_node::agents::mcp::command_name;
use beep_proto::{
    CreateProject, FoundPlace, FoundProject, ImportedProject, NewRepository, ProjectFolder,
    node::tools::Reply,
};
use futures::{StreamExt, future::join_all, stream};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, future::Future, sync::Arc};
use tauri::{AppHandle, Manager};

/// Projects imported at once, as the Add a project sheet does
/// (`IMPORTS_AT_ONCE` in `addproject-state.ts`).
const IMPORTS_AT_ONCE: usize = 3;

/// Why a name `import` was given is not imported.
const NOT_FOUND: &str = "No checkout found has that name; call found first.";
/// Why a project with no checkout on the machines named is not imported.
const NOT_THERE: &str = "It has no checkout on the machines named.";

/// What the assistant asks, as the tool's arguments name it. The schema
/// lets any field come with any action, so others are ignored.
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    Hosts,
    Create {
        name: String,
        #[serde(default)]
        machines: Vec<String>,
        folder: Option<String>,
        repository: Option<RepositoryArg>,
    },
    Clone {
        url: String,
        #[serde(default)]
        machines: Vec<String>,
        name: Option<String>,
        folder: Option<String>,
    },
    Found {
        #[serde(default)]
        machines: Vec<String>,
    },
    Import {
        projects: Vec<String>,
        #[serde(default)]
        machines: Vec<String>,
    },
}

/// The repository `create` makes, private unless the assistant says not.
#[derive(Deserialize)]
struct RepositoryArg {
    host: String,
    owner: String,
    private: Option<bool>,
}

impl From<RepositoryArg> for NewRepository {
    fn from(arg: RepositoryArg) -> Self {
        Self {
            host: arg.host,
            owner: arg.owner,
            private: arg.private.unwrap_or(true),
        }
    }
}

/// A project that landed on a machine, as a reply names it.
#[derive(Serialize)]
struct Landed {
    project: String,
    machine: String,
    folder: String,
    cloned: bool,
}

impl From<ImportedProject> for Landed {
    fn from(imported: ImportedProject) -> Self {
        Self {
            project: imported.project,
            machine: imported.machine_name,
            folder: imported.folder,
            cloned: imported.cloned,
        }
    }
}

/// The answer to one call, as JSON text of what happened.
pub(crate) async fn answer(app: &AppHandle, args: &str) -> Reply {
    match run(app, args).await {
        Ok(reply) => Reply::Done {
            text: reply.to_string(),
            failed: false,
        },
        Err(text) => Reply::Done { text, failed: true },
    }
}

async fn run(app: &AppHandle, args: &str) -> Result<Value, String> {
    let action: Action =
        serde_json::from_str(args).map_err(|e| format!("The arguments do not fit: {e}"))?;
    let (Some(fleet), Some(session), Some(held)) = (
        app.try_state::<Arc<Fleet>>(),
        app.try_state::<Arc<Session>>(),
        app.try_state::<MachineLists>(),
    ) else {
        return Err("The desktop's mesh did not start.".to_owned());
    };
    let desk = desk(&session).await.map_err(|e| e.to_string())?;

    match action {
        Action::Hosts => {
            let forges = desk.forges().await.map_err(|e| e.to_string())?;
            let rows: Vec<ForgeRow> = forges.into_iter().map(ForgeRow::from).collect();

            Ok(json!(rows))
        }
        Action::Create {
            name,
            machines,
            folder,
            repository,
        } => {
            let ids = machine_ids(&fleet, &machines)?;
            let request = CreateProject {
                folders: folders(&ids, folder.as_deref()),
                machine_ids: ids,
                name,
                repository: repository.map(NewRepository::from),
            };
            let created = desk.create(&request).await.map_err(|e| e.to_string())?;

            Ok(json!(CreatedRow::from(created)))
        }
        Action::Clone {
            url,
            machines,
            name,
            folder,
        } => {
            let ids = machine_ids(&fleet, &machines)?;
            Ok(clone(&desk, &url, &ids, name.as_deref(), folder.as_deref()).await)
        }
        Action::Found { machines } => {
            let machines = if machines.is_empty() {
                fleet.online()
            } else {
                machine_ids(&fleet, &machines)?
                    .iter()
                    .filter_map(|id| fleet.member(id))
                    .collect()
            };
            Ok(found(&fleet, &desk, &held, machines).await)
        }
        Action::Import { projects, machines } => {
            let ids = machine_ids(&fleet, &machines)?;
            let found = desk.group_found(held.found.last());
            let import = |repo: String, place: FoundPlace| {
                let desk = &desk;
                async move {
                    let job = ImportJob {
                        repo: &repo,
                        machine_id: Some(&place.machine_id),
                        folder: Some(&place.folder),
                        parent: None,
                        worktrees: &[],
                    };
                    desk.import(job).await.map_err(|e| e.to_string())
                }
            };

            Ok(import_found(&found, &projects, &ids, import).await)
        }
    }
}

/// The machine ids `names` name, by name as `machines list` gives it or by id.
fn machine_ids(fleet: &Fleet, names: &[String]) -> Result<Vec<String>, String> {
    let machines = fleet.machines();

    names
        .iter()
        .map(|named| {
            let wanted = command_name(named);
            machines
                .iter()
                .find(|status| {
                    status.device.id.to_string() == named.trim()
                        || command_name(&status.device.name) == wanted
                })
                .map(|status| status.device.id.to_string())
                .ok_or_else(|| format!("There is no machine {named}."))
        })
        .collect()
}

/// `folder` on each machine, when one is given.
fn folders(machine_ids: &[String], folder: Option<&str>) -> Vec<ProjectFolder> {
    let Some(folder) = folder.map(str::trim).filter(|folder| !folder.is_empty()) else {
        return Vec::new();
    };

    machine_ids
        .iter()
        .map(|machine_id| ProjectFolder {
            machine_id: machine_id.clone(),
            folder: folder.to_owned(),
        })
        .collect()
}

/// Clones `url` onto each machine, or the one that fits best, under
/// `folder`, then names the project `name`.
async fn clone(
    desk: &Desk,
    url: &str,
    machine_ids: &[String],
    name: Option<&str>,
    folder: Option<&str>,
) -> Value {
    let targets: Vec<Option<&str>> = if machine_ids.is_empty() {
        vec![None]
    } else {
        machine_ids.iter().map(|id| Some(id.as_str())).collect()
    };
    let jobs = targets.iter().map(|machine_id| {
        desk.import(ImportJob {
            repo: url,
            machine_id: *machine_id,
            folder: None,
            parent: folder,
            worktrees: &[],
        })
    });
    let (mut imported, mut failed) = (Vec::new(), Vec::new());

    for (machine_id, done) in targets.iter().zip(join_all(jobs).await) {
        match done {
            Ok(project) => imported.push(Landed::from(project)),
            Err(e) => failed.push(json!({ "machine": machine_id, "why": e.to_string() })),
        }
    }

    let named = match (name.map(str::trim), imported.first()) {
        (Some(name), Some(first)) if !name.is_empty() => desk
            .rename_project(&first.project, name, &[])
            .await
            .err()
            .map(|e| format!("It kept its name: {e}")),
        _ => None,
    };

    json!({ "imported": imported, "failed": failed, "name_failed": named })
}

/// Has `machines` look for checkouts again, then gives those that are no
/// project yet, with the machines that could not say.
async fn found(
    fleet: &Fleet,
    desk: &Desk,
    held: &MachineLists,
    machines: Vec<EndpointId>,
) -> Value {
    let asks = machines
        .iter()
        .map(|machine| direct::found_on(fleet, *machine));
    let mut problems = Vec::new();

    for (machine, done) in machines.iter().zip(join_all(asks).await) {
        match done {
            Ok(found) => held.found.keep(found),
            Err(why) => problems.push(json!({ "machine": fleet.name_of(*machine), "why": why })),
        }
    }

    let rows: Vec<FoundRow> = desk
        .group_found(held.found.last())
        .into_iter()
        .map(FoundRow::from)
        .collect();

    json!({ "found": rows, "problems": problems })
}

/// Makes projects of the found checkouts `names` names, a few at a time,
/// with `import` linking each checkout, on `machine_ids` when given.
async fn import_found<F, Fut>(
    found: &[FoundProject],
    names: &[String],
    machine_ids: &[String],
    import: F,
) -> Value
where
    F: Fn(String, FoundPlace) -> Fut,
    Fut: Future<Output = Result<ImportedProject, String>>,
{
    let mut seen = HashSet::new();
    let wanted: Vec<String> = names
        .iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| seen.insert(name.to_ascii_lowercase()))
        .collect();
    let import = &import;
    let outcomes: Vec<Outcome> = stream::iter(wanted)
        .map(|name| async move {
            let Some(project) = found.iter().find(|project| {
                project.project.eq_ignore_ascii_case(&name) || project.remote == name
            }) else {
                return Outcome::Skipped(name, NOT_FOUND);
            };

            let places = project
                .places
                .iter()
                .filter(|place| machine_ids.is_empty() || machine_ids.contains(&place.machine_id));
            let (mut landed, mut why) = (Vec::new(), None);

            // One checkout after another, as the sheet links them.
            for place in places {
                match import(project.remote.clone(), place.clone()).await {
                    Ok(imported) => landed.push(Landed::from(imported)),
                    Err(e) => why = why.or(Some(e)),
                }
            }

            match (landed.is_empty(), why) {
                (false, _) => Outcome::Imported(landed),
                (true, Some(why)) => Outcome::Failed(project.project.clone(), why),
                (true, None) => Outcome::Skipped(project.project.clone(), NOT_THERE),
            }
        })
        .buffered(IMPORTS_AT_ONCE)
        .collect()
        .await;

    let (mut imported, mut skipped, mut failed) = (Vec::new(), Vec::new(), Vec::new());

    for outcome in outcomes {
        match outcome {
            Outcome::Imported(landed) => imported.extend(landed),
            Outcome::Skipped(project, why) => {
                skipped.push(json!({ "project": project, "why": why }))
            }
            Outcome::Failed(project, why) => failed.push(json!({ "project": project, "why": why })),
        }
    }

    json!({ "imported": imported, "skipped": skipped, "failed": failed })
}

/// What `import_found` did with one name.
enum Outcome {
    Imported(Vec<Landed>),
    Skipped(String, &'static str),
    Failed(String, String),
}

#[cfg(test)]
#[path = "../tests/projects_agent.rs"]
mod tests;
