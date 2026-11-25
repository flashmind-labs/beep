//! Importing a repository onto a machine and making a new project: picking the machine, the
//! clone or link, and recording it. What runs around it (starting sync, looking for
//! checkouts elsewhere) is behind [`Effects`].

use super::{
    ImportError,
    clone::run_clone,
    folder::{self, FolderPath, look},
    fork::UsedFork,
    hosts::{Hosts, create_repository, find_fork, forgejo_of},
    link::link_fork,
    machine::Machine,
    place::{self, Candidate},
    preview::existing_checkout,
    records::{ImportRow, NewImport},
    repo::{RepoRef, valid_part},
    store::ImportStore,
};
use crate::credentials::Opened;
use anyhow::Result;
use async_trait::async_trait;
use beep_proto::{CheckoutFailure, CreateProject, CreatedProject, ImportedProject, ProjectFolder};
use futures::{StreamExt, stream};

/// How many machines clone at once.
const CLONES_AT_ONCE: usize = 4;
/// The owner of a project with no git host, as projects are named `owner/repo`.
const LOCAL_OWNER: &str = "local";

/// What runs around an import.
#[async_trait]
pub trait Effects: Send + Sync {
    /// An import landed on `machine`: look for checkouts on the user's other machines.
    async fn imported(&self, machine: &str);

    /// Starts syncing the checkouts with each other; a machine that refuses is listed in
    /// `failed` and keeps its copy.
    async fn join_sync(
        &self,
        imported: &[ImportedProject],
        failed: &mut Vec<CheckoutFailure>,
    ) -> Result<()>;
}

/// What an import reads and writes.
pub struct Env<'a> {
    pub store: &'a dyn ImportStore,
    pub hosts: &'a dyn Hosts,
    pub effects: &'a dyn Effects,
    /// The user's git credentials, opened.
    pub credentials: &'a [Opened],
    /// Every machine of the user's, online or not.
    pub machines: &'a [Machine],
}

/// One import, as the sheet or the tool asks for it.
#[derive(Debug, Clone, Copy)]
pub struct Import<'a> {
    /// `org/repo` or the repository's URL.
    pub repo: &'a str,
    /// The machine's id; `None` lets `place::choose` pick.
    pub machine_id: Option<&'a str>,
    /// Where it goes, absolute or `~/...`; `None` or blank to go under `parent`.
    pub folder: Option<&'a str>,
    /// The folder it goes under when `folder` is not given; `None` or blank for the machine's
    /// projects folder.
    pub parent: Option<&'a str>,
    /// How it goes under `parent`.
    pub nesting: Nesting,
}

/// How an import goes under its parent folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nesting {
    /// `<parent>/<org>/<repo>`, as imports go.
    Org,
    /// `<parent>/<repo>`, as new projects go.
    Flat,
}

/// The machine `machine_id` names, or the one `place::choose` picks.
fn choose_machine<'a>(
    machines: &'a [Machine],
    machine_id: Option<&str>,
) -> Result<&'a Machine, ImportError> {
    let candidates: Vec<Candidate<'_>> = machines
        .iter()
        .map(|machine| Candidate {
            id: &machine.id,
            name: &machine.name,
            online: machine.online,
            shell: machine.shell.is_some(),
        })
        .collect();
    Ok(&machines[place::choose(&candidates, machine_id)?])
}

/// The absolute folder `parts` name under `parent` on `machine`, or under its projects folder when
/// `parent` is not given; inside it when another folder already holds that name.
async fn under_parent(
    machine: &Machine,
    parent: Option<&str>,
    parts: &[&str],
) -> Result<String, ImportError> {
    let given = parent.map(str::trim).filter(|parent| !parent.is_empty());
    let parent = match given {
        Some(parent) => parent.to_owned(),
        None => place::projects_root(machine.folders())
            .ok_or_else(|| ImportError::NoHome(machine.name.clone()))?,
    };
    let joined = place::join(&parent, parts);
    let path = match FolderPath::parse(&joined).and_then(|path| path.ok_or(ImportError::BadFolder))
    {
        Ok(path) => path,
        // The machine's own folder in a form only it reads, such as a Windows path.
        Err(_) if given.is_none() => return Ok(joined),
        Err(e) => return Err(e),
    };
    let seen = look(machine.shell()?, &machine.name, &path).await?;
    let name = parts.last().copied().unwrap_or_default();
    Ok(folder::destination(&seen, name))
}

/// The recorded import as the wire shows it.
pub fn project(row: ImportRow, machine_name: String, cloned: bool) -> ImportedProject {
    ImportedProject {
        project: row.project,
        url: row.url,
        machine_id: row.endpoint,
        machine_name,
        folder: row.folder,
        cloned,
        imported_at: row.imported_at.into(),
        display_name: None,
    }
}

/// Imports `job.repo` onto its machine: clones (the fork, when any) or links an existing
/// checkout, and records it.
pub async fn import(env: &Env<'_>, job: Import<'_>) -> Result<ImportedProject, ImportError> {
    let repo = RepoRef::parse(job.repo, &forgejo_of(env.credentials))?;
    let picked = match job.folder {
        Some(folder) => FolderPath::parse(folder)?,
        None => None,
    };
    let machine = choose_machine(env.machines, job.machine_id)?;
    let fork = find_fork(env.hosts, env.credentials, &repo).await;
    let seen = match &picked {
        Some(picked) => Some(look(machine.shell()?, &machine.name, picked).await?),
        None => None,
    };
    let existing = if seen.is_none() {
        existing_checkout(machine, &repo, fork.as_ref().map(|fork| &fork.repo)).await
    } else {
        None
    };
    let token = super::hosts::credential_of(&repo, env.credentials).map(|opened| &opened.secret);
    let (folder, cloned) = match existing {
        Some(found) => (found.folder, false),
        None => {
            let folder = match &seen {
                Some(seen) => folder::destination(seen, repo.name()),
                None => {
                    let parts = match job.nesting {
                        Nesting::Org => vec![repo.org(), repo.name()],
                        Nesting::Flat => vec![repo.name()],
                    };
                    under_parent(machine, job.parent, &parts).await?
                }
            };
            let cloned = run_clone(
                machine.shell()?,
                &machine.name,
                &repo,
                fork.as_ref(),
                &folder,
                token,
            )
            .await?;
            (folder, cloned)
        }
    };

    if !cloned && let Some(fork) = &fork {
        link_fork(machine, &repo, fork, &folder, token).await;
    }

    let saved = env
        .store
        .save_import(NewImport {
            project: &repo.project(),
            endpoint: &machine.id,
            folder: &folder,
            url: &repo.web_url(),
        })
        .await?;
    let fork_name = fork.as_ref().map(|fork: &UsedFork| &fork.found.name);
    env.store
        .set_import_fork(&saved.project, &machine.id, fork_name)
        .await?;
    log::info!(
        "project imported; project={} node={} cloned={cloned} fork={}",
        saved.project,
        machine.id,
        fork.is_some()
    );
    env.effects.imported(&machine.id).await;
    Ok(project(saved, machine.name.clone(), cloned))
}

/// Makes the project `request` asks for. Every machine is checked first, so an offline one stops it
/// before anything exists.
pub async fn create_project(
    env: &Env<'_>,
    request: &CreateProject,
) -> Result<CreatedProject, ImportError> {
    let name = request.name.trim();

    if !valid_part(name) || name.ends_with(".git") {
        return Err(ImportError::BadName);
    }

    let mut ids: Vec<&str> = Vec::new();

    for id in request.machine_ids.iter().map(|id| id.trim()) {
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
    }

    if ids.is_empty() {
        return Err(ImportError::NoMachine);
    }

    if request.repository.is_none() && ids.len() > 1 {
        return Err(ImportError::LocalOnOne);
    }

    let mut chosen = Vec::with_capacity(ids.len());

    for id in ids {
        chosen.push(choose_machine(env.machines, Some(id))?);
    }

    match &request.repository {
        Some(new) => {
            let repo = create_repository(env.hosts, env.credentials, new, name).await?;
            Ok(clone_onto(env, &repo, &chosen, &request.folders).await)
        }
        None => create_local(env, name, chosen, &request.folders).await,
    }
}

/// Imports `repo` onto each machine, under its folder in `folders` when given, else its projects
/// folder; those that landed join sync. A machine that fails is listed and the repository stays.
async fn clone_onto(
    env: &Env<'_>,
    repo: &RepoRef,
    machines: &[&Machine],
    folders: &[ProjectFolder],
) -> CreatedProject {
    let url = repo.web_url();
    // Collected first: a stream's `map` closure over borrows is not `Send` enough for the router.
    let clones: Vec<_> = machines
        .iter()
        .map(|machine| {
            import(
                env,
                Import {
                    repo: &url,
                    machine_id: Some(&machine.id),
                    folder: None,
                    parent: folder_of(folders, &machine.id),
                    nesting: Nesting::Flat,
                },
            )
        })
        .collect();
    let answers: Vec<_> = stream::iter(clones)
        .buffered(CLONES_AT_ONCE)
        .collect()
        .await;
    let mut imported = Vec::new();
    let mut failed = Vec::new();

    for (machine, answer) in machines.iter().zip(answers) {
        match answer {
            Ok(landed) => imported.push(landed),
            Err(e) => {
                log::info!(
                    "clone failed; project={} node={}",
                    repo.project(),
                    machine.id
                );
                failed.push(CheckoutFailure {
                    machine_id: machine.id.clone(),
                    machine_name: machine.name.clone(),
                    why: e.to_string(),
                });
            }
        }
    }

    if let Err(e) = env.effects.join_sync(&imported, &mut failed).await {
        log::warn!(
            "new project not synced; project={} error={e:#}",
            repo.project()
        );
    }
    CreatedProject {
        project: repo.project(),
        url,
        imported,
        failed,
    }
}

/// The folder `folders` names for the machine `id`.
fn folder_of<'a>(folders: &'a [ProjectFolder], id: &str) -> Option<&'a str> {
    folders
        .iter()
        .find(|folder| folder.machine_id == id)
        .map(|folder| folder.folder.as_str())
}

/// `git init` under the machine's folder in `folders`, else its projects folder, imported there
/// with no git host.
async fn create_local(
    env: &Env<'_>,
    name: &str,
    mut chosen: Vec<&Machine>,
    folders: &[ProjectFolder],
) -> Result<CreatedProject, ImportError> {
    let Some(machine) = chosen.pop() else {
        return Err(ImportError::NoMachine);
    };
    let project_name = format!("{LOCAL_OWNER}/{name}");

    if env
        .store
        .imports()
        .await?
        .iter()
        .any(|one| one.project.eq_ignore_ascii_case(&project_name))
    {
        return Err(ImportError::ProjectExists(name.to_owned()));
    }

    let folder = under_parent(machine, folder_of(folders, &machine.id), &[name]).await?;
    super::init::init(machine.shell()?, &machine.name, &folder).await?;
    let saved = env
        .store
        .save_import(NewImport {
            project: &project_name,
            endpoint: &machine.id,
            folder: &folder,
            url: "",
        })
        .await?;
    log::info!(
        "local project created; project={project_name} node={}",
        machine.id
    );
    Ok(CreatedProject {
        project: project_name,
        url: String::new(),
        imported: vec![project(saved, machine.name.clone(), true)],
        failed: Vec::new(),
    })
}

#[cfg(test)]
#[path = "../tests/imports_flow.rs"]
mod tests;
