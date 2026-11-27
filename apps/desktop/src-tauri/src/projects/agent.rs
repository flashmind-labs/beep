//! The project tools of the Beep MCP server (`projects_hosts`,
//! `projects_create`...), as this desktop answers them for the home assistant: the Git hosts, making and cloning projects, and making
//! projects of the checkouts the machines have. Each action does what the
//! Add a project and New project sheets do.

use super::create::CreatedRow;
use crate::{
    fleet::{Fleet, files as direct, projects::desk},
    machines::list::MachineLists,
};
use beep_fleet::{Desk, ImportJob, Session};
use beep_node::tools::{ProjectTools, command_name};
use beep_proto::{
    CreateProject, FoundPlace, FoundProject, ImportedProject, NewRepository, ProjectFolder,
    node::tools::Reply,
};
use futures::{
    StreamExt,
    future::{BoxFuture, join_all},
    stream,
};
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

/// What the assistant asks: the tool's action, which its node adds, and the
/// tool's arguments.
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
    JoinTeam {
        ticket: String,
        machine: Option<String>,
        folder: Option<String>,
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

/// A checkout or clone that did not land, on the machine by its name; no
/// machine when Beep was to pick one.
#[derive(Serialize)]
struct Failure {
    project: String,
    machine: Option<String>,
    why: String,
}

/// The desktop's answer to the assistant's project tools, for its node.
pub(crate) fn tools(app: &AppHandle) -> Arc<dyn ProjectTools> {
    Arc::new(Tools(app.clone()))
}

struct Tools(AppHandle);

impl ProjectTools for Tools {
    fn call(&self, args: String) -> BoxFuture<'_, Reply> {
        Box::pin(async move { answer(&self.0, &args).await })
    }
}

/// The answer to one call, as JSON text of what happened.
async fn answer(app: &AppHandle, args: &str) -> Reply {
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

            Ok(json!(forges))
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
            Ok(clone(
                &fleet,
                &desk,
                &url,
                &ids,
                name.as_deref(),
                folder.as_deref(),
            )
            .await)
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
        Action::JoinTeam {
            ticket,
            machine,
            folder,
        } => {
            let machine_id = match machine.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                Some(name) => machine_ids(&fleet, &[name.to_owned()])?.into_iter().next(),
                None => None,
            };
            let folder = folder
                .as_deref()
                .map(str::trim)
                .filter(|dir| !dir.is_empty());
            let (repo, existing) = desk.team_ticket(&ticket).map_err(|e| e.to_string())?;
            let prepared = super::team::ensure_team_project(
                &fleet,
                &desk,
                &held,
                &repo,
                existing,
                machine_id.as_deref(),
                folder,
            )
            .await
            .map_err(|e| e.to_string())?;
            let record = desk.team_join(&ticket).await.map_err(|e| e.to_string())?;

            if let Ok(invites) = desk.team_invites().await {
                for invite in invites.into_iter().filter(|invite| invite.repo == repo) {
                    if let Err(e) = desk.team_invite_drop(invite).await {
                        log::warn!("invite drop after join_team: {e}");
                    }
                }
            }

            let me = record
                .members
                .iter()
                .find(|member| member.mesh == desk.mesh_id())
                .map(|member| member.login.to_string());
            let members: Vec<String> = record
                .members
                .iter()
                .map(|member| member.login.to_string())
                .collect();

            Ok(json!({
                "repo": record.repo.to_string(),
                "project": prepared.project,
                "machine": prepared.machine_name,
                "folder": prepared.folder,
                "added_project": prepared.added,
                "cloned": prepared.cloned,
                "me": me,
                "members": members,
            }))
        }
    }
}

/// The machine ids `names` name, by name as `list_machines` gives it or by
/// id. Phones are no such machines: they hold no checkouts.
fn machine_ids(fleet: &Fleet, names: &[String]) -> Result<Vec<String>, String> {
    let machines = fleet
        .mesh()
        .map(|mesh| mesh.code_hosts())
        .unwrap_or_default();

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
    fleet: &Fleet,
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
            Err(e) => failed.push(Failure {
                project: url.to_owned(),
                machine: machine_id
                    .and_then(|id| fleet.member(id))
                    .map(|machine| fleet.name_of(machine)),
                why: e.to_string(),
            }),
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

    let found = desk.group_found(held.found.last());

    json!({ "found": found, "problems": problems })
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
    // A name and its remote resolve to the same project: keep it once, by remote.
    let mut seen = HashSet::new();
    let wanted: Vec<Result<usize, String>> = names
        .iter()
        .map(|name| {
            let name = name.trim();
            found
                .iter()
                .position(|project| {
                    project.project.eq_ignore_ascii_case(name) || project.remote == name
                })
                .ok_or_else(|| name.to_owned())
        })
        .filter(|wanted| match wanted {
            Ok(at) => seen.insert(found[*at].remote.clone()),
            Err(name) => seen.insert(name.to_ascii_lowercase()),
        })
        .collect();
    let import = &import;
    let outcomes: Vec<Outcome> = stream::iter(wanted)
        .map(|wanted| async move {
            let project = match wanted {
                Ok(at) => &found[at],
                Err(name) => return Outcome::Skipped(name, NOT_FOUND),
            };

            let places = project
                .places
                .iter()
                .filter(|place| machine_ids.is_empty() || machine_ids.contains(&place.machine_id));
            let (mut landed, mut failed) = (Vec::new(), Vec::new());

            // One checkout after another, as the sheet links them.
            for place in places {
                match import(project.remote.clone(), place.clone()).await {
                    Ok(imported) => landed.push(Landed::from(imported)),
                    Err(why) => failed.push(Failure {
                        project: project.project.clone(),
                        machine: Some(place.machine_name.clone()),
                        why,
                    }),
                }
            }

            if landed.is_empty() && failed.is_empty() {
                Outcome::Skipped(project.project.clone(), NOT_THERE)
            } else {
                Outcome::Linked { landed, failed }
            }
        })
        .buffered(IMPORTS_AT_ONCE)
        .collect()
        .await;

    let (mut imported, mut skipped, mut failed) = (Vec::new(), Vec::new(), Vec::new());

    for outcome in outcomes {
        match outcome {
            Outcome::Linked {
                landed,
                failed: missed,
            } => {
                imported.extend(landed);
                failed.extend(missed);
            }
            Outcome::Skipped(project, why) => {
                skipped.push(json!({ "project": project, "why": why }))
            }
        }
    }

    json!({ "imported": imported, "skipped": skipped, "failed": failed })
}

/// What `import_found` did with one name.
enum Outcome {
    /// The checkouts it linked and those that failed.
    Linked {
        landed: Vec<Landed>,
        failed: Vec<Failure>,
    },
    Skipped(String, &'static str),
}

#[cfg(test)]
#[path = "../tests/projects_agent.rs"]
mod tests;
