//! Importing a repository onto a machine and making a new project: picking the machine, the
//! clone or link, and recording it. What the server does around it (starting sync, looking for
//! checkouts elsewhere) is behind [`Effects`].

use super::{
    ImportError,
    clone::run_clone,
    folder::{self, FolderPath, look},
    fork::UsedFork,
    hosts::{Hosts, create_repository, find_fork, forgejo_of},
    link::link_fork,
    machine::Machine,
    place::{self, Candidate, project_folder},
    preview::existing_checkout,
    records::{ImportRow, NewImport},
    repo::{RepoRef, valid_part},
    store::ImportStore,
};
use crate::credentials::Opened;
use anyhow::Result;
use async_trait::async_trait;
use beep_proto::{CheckoutFailure, CreateProject, CreatedProject, ImportedProject};
use futures::{StreamExt, stream};

/// How many machines clone at once.
const CLONES_AT_ONCE: usize = 4;
/// The owner of a project with no git host, as projects are named `owner/repo`.
const LOCAL_OWNER: &str = "local";

/// What runs around an import and differs between the server and the desktop.
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
    /// Where it goes, absolute or `~/...`; `None` or blank for the machine's projects folder.
    pub folder: Option<&'a str>,
    /// The folder it goes under as `<parent>/<org>/<repo>` when `folder` is not given.
    pub parent: Option<&'a str>,
}

/// The machine `machine_id` names, or the one `place::choose` picks.
pub fn choose_machine<'a>(
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
            workspace: machine.online && machine.workspace.is_some(),
        })
        .collect();
    Ok(&machines[place::choose(&candidates, machine_id)?])
}

/// `<parent>/<org>/<repo>`; blank when `parent` is, so the machine's projects folder is used.
fn under_parent(parent: &str, repo: &RepoRef) -> String {
    let parent = parent.trim().trim_end_matches('/');
    if parent.is_empty() {
        return String::new();
    }
    format!("{parent}/{}/{}", repo.org(), repo.name())
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
    }
}

/// Imports `job.repo` onto its machine: clones (the fork, when any) or links an existing
/// checkout, and records it.
pub async fn import(env: &Env<'_>, job: Import<'_>) -> Result<ImportedProject, ImportError> {
    let repo = RepoRef::parse(job.repo, &forgejo_of(env.credentials))?;
    let folder = match job.folder.filter(|folder| !folder.trim().is_empty()) {
        Some(folder) => Some(folder.to_owned()),
        None => job.parent.map(|parent| under_parent(parent, &repo)),
    };
    let picked = match folder.as_deref() {
        Some(folder) => FolderPath::parse(folder)?,
        None => None,
    };
    let machine = choose_machine(env.machines, job.machine_id)?;
    let fork = find_fork(env.hosts, env.credentials, &repo).await;
    let seen = match &picked {
        Some(picked) => Some(look(machine.shell()?, &machine.name, picked).await?),
        None => None,
    };
    let existing = if seen.as_ref().is_none_or(|seen| !seen.exists) {
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
                None => place::checkout_folder(machine.folders(), &repo)
                    .ok_or_else(|| ImportError::NoHome(machine.name.clone()))?,
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
            Ok(clone_onto(env, &repo, &chosen, None).await)
        }
        None => create_local(env, name, chosen).await,
    }
}

/// Imports `repo` onto each machine, under `parent` when given, else its projects folder; those that
/// landed join sync. A machine that fails is listed and the repository stays.
pub async fn clone_onto(
    env: &Env<'_>,
    repo: &RepoRef,
    machines: &[&Machine],
    parent: Option<&str>,
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
                    parent,
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

/// `git init` in the machine's projects folder, imported there with no git host.
async fn create_local(
    env: &Env<'_>,
    name: &str,
    mut chosen: Vec<&Machine>,
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
    let folder = project_folder(machine.folders(), &[name])
        .ok_or_else(|| ImportError::NoHome(machine.name.clone()))?;
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
mod tests {
    use super::*;

    #[test]
    fn a_parent_takes_the_owner_and_name() {
        let repo = RepoRef::parse("acme/widget", &[]).expect("org/repo parses");
        assert_eq!(under_parent("~/Projects/", &repo), "~/Projects/acme/widget");
        assert_eq!(under_parent("  ", &repo), "");
    }

    use crate::{ShellOutput, model::ForgejoUrl};
    use reqwest::Client;
    use std::sync::{Arc, Mutex};
    use tokio::sync::Semaphore;

    /// A shell that answers each script by what it is for.
    struct Scripted;

    #[async_trait]
    impl crate::Shell for Scripted {
        async fn run(&self, script: &str, _: std::time::Duration) -> Result<ShellOutput> {
            // Looking for checkouts finds none; the clone lands.
            Ok(if script.contains("BEEP_IMPORT:") {
                ShellOutput::exited(0, "\nBEEP_IMPORT: cloned\n")
            } else {
                ShellOutput::exited(0, "")
            })
        }
    }

    #[derive(Default)]
    struct Memory {
        saved: Mutex<Vec<(String, String, String)>>,
    }

    #[async_trait]
    impl ImportStore for Memory {
        async fn imports(&self) -> Result<Vec<ImportRow>> {
            Ok(Vec::new())
        }
        async fn save_import(&self, import: NewImport<'_>) -> Result<ImportRow> {
            self.saved.lock().expect("not poisoned").push((
                import.project.to_owned(),
                import.endpoint.to_owned(),
                import.folder.to_owned(),
            ));
            Ok(ImportRow {
                project: import.project.to_owned(),
                endpoint: import.endpoint.to_owned(),
                folder: import.folder.to_owned(),
                url: import.url.to_owned(),
                imported_at: chrono::Utc::now(),
                fork: None,
            })
        }
        async fn link_import(&self, _: NewImport<'_>) -> Result<bool> {
            Ok(false)
        }
        async fn set_import_fork(
            &self,
            _: &str,
            _: &str,
            _: Option<&crate::model::RepoName>,
        ) -> Result<()> {
            Ok(())
        }
    }

    struct NoHosts(Arc<Semaphore>);

    #[async_trait]
    impl Hosts for NoHosts {
        fn client(&self) -> Result<Client> {
            Ok(Client::new())
        }
        fn calls(&self) -> &Arc<Semaphore> {
            &self.0
        }
        async fn is_public(&self, _: &ForgejoUrl) -> bool {
            false
        }
    }

    #[derive(Default)]
    struct Noted(Mutex<Vec<String>>);

    #[async_trait]
    impl Effects for Noted {
        async fn imported(&self, machine: &str) {
            self.0
                .lock()
                .expect("not poisoned")
                .push(machine.to_owned());
        }
        async fn join_sync(
            &self,
            _: &[ImportedProject],
            _: &mut Vec<CheckoutFailure>,
        ) -> Result<()> {
            Ok(())
        }
    }

    fn box_machine() -> Machine {
        Machine {
            id: "box-id".into(),
            name: "build-box".into(),
            online: true,
            os: Some("linux".into()),
            home: Some("/home/me".into()),
            workspace: None,
            shell: Some(Arc::new(Scripted)),
            sessions: None,
        }
    }

    #[tokio::test]
    async fn an_import_records_what_the_fake_shell_cloned() {
        let store = Memory::default();
        let hosts = NoHosts(Arc::new(Semaphore::new(1)));
        let effects = Noted::default();
        let machines = [box_machine()];
        let env = Env {
            store: &store,
            hosts: &hosts,
            effects: &effects,
            credentials: &[],
            machines: &machines,
        };
        let landed = import(
            &env,
            Import {
                repo: "acme/widget",
                machine_id: Some("box-id"),
                folder: None,
                parent: None,
            },
        )
        .await
        .expect("the import lands");
        assert!(landed.cloned);
        assert_eq!(landed.project, "acme/widget");
        assert_eq!(landed.folder, "/home/me/projects/acme/widget");
        assert_eq!(
            store.saved.lock().expect("not poisoned").as_slice(),
            [(
                "acme/widget".to_owned(),
                "box-id".to_owned(),
                "/home/me/projects/acme/widget".to_owned()
            )]
        );
        assert_eq!(*effects.0.lock().expect("not poisoned"), ["box-id"]);
    }
}
