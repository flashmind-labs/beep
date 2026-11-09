//! Discovering and importing repositories through the machines' shells.

use super::{checkouts::FALLBACK_DEFAULT, store::MeshStore};
use crate::{desk::Desk, error::Result};
use async_trait::async_trait;
use beep_projects::{
    Shell,
    branches::add_worktree,
    imports::{
        flow::{self, Effects, Env, Import},
        found::{self, MachineFound},
        host_repos::read_hosts,
        hosts::{find_fork, forgejo_of, project_forges},
        machine::Machine,
        preview as previews,
        repo::RepoRef,
    },
    model::branch_name,
};
use beep_proto::{
    BranchPlace, CheckoutFailure, CreateProject, CreatedProject, FoundProject, HostRepos,
    ImportFork, ImportPreview, ImportedProject, ProjectForge,
};
use iroh::EndpointId;
use std::{collections::HashMap, sync::Arc};

const MAX_IMPORT_WORKTREES: usize = 20;
const MAX_REPOS: usize = 100;

/// After an import: nothing more. `MeshStore` writes `sync/<name>` with each checkout it saves.
struct MeshEffects;

#[async_trait]
impl Effects for MeshEffects {
    /// The desktop does not look through the other machines on its own; the Add a project sheet
    /// finds checkouts there when it is opened.
    async fn imported(&self, _: &str) {}

    async fn join_sync(
        &self,
        _: &[ImportedProject],
        _: &mut Vec<CheckoutFailure>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// An import as the Add a project sheet asks for it.
pub struct ImportJob<'a> {
    pub repo: &'a str,
    pub machine_id: Option<&'a str>,
    pub folder: Option<&'a str>,
    pub parent: Option<&'a str>,
    /// Branches to give a worktree after, on this machine or another.
    pub worktrees: &'a [BranchPlace],
}

impl Desk {
    /// The imported projects, one row per machine that has one.
    pub async fn imported(&self) -> Vec<ImportedProject> {
        let names: HashMap<EndpointId, String> = self
            .mesh
            .code_hosts()
            .into_iter()
            .map(|status| (status.device.id, status.device.name))
            .collect();
        MeshStore::new(&self.mesh)
            .rows()
            .into_iter()
            .filter_map(|row| {
                let id: EndpointId = row.endpoint.parse().ok()?;
                let name = names.get(&id)?.clone();
                Some(flow::project(row, name, false))
            })
            .collect()
    }

    /// Clones or finds `job.repo` on its machine and records it, then adds the worktrees asked for.
    pub async fn import(&self, job: ImportJob<'_>) -> Result<ImportedProject> {
        let store = MeshStore::new(&self.mesh);
        let (credentials, machines) = (self.opened(), self.machines().await);
        let env = Env {
            store: &store,
            hosts: &self.hosts,
            effects: &MeshEffects,
            credentials: &credentials,
            machines: &machines,
        };
        let imported = flow::import(
            &env,
            Import {
                repo: job.repo,
                machine_id: job.machine_id,
                folder: job.folder,
                parent: job.parent,
            },
        )
        .await?;
        self.add_import_worktrees(&store, &machines, &imported, job.worktrees)
            .await;
        Ok(imported)
    }

    /// One worktree that fails is logged and the import stands.
    async fn add_import_worktrees(
        &self,
        store: &MeshStore,
        machines: &[Machine],
        imported: &ImportedProject,
        worktrees: &[BranchPlace],
    ) {
        let usable: Vec<Machine> = machines
            .iter()
            .filter(|machine| machine.shell.is_some())
            .cloned()
            .collect();
        let servers = forgejo_of(&self.opened());

        for place in worktrees.iter().take(MAX_IMPORT_WORKTREES) {
            let on = place.machine_id.as_deref().unwrap_or(&imported.machine_id);
            let Some(branch) = branch_name(&place.branch) else {
                log::info!("import: not a branch name for a worktree");
                continue;
            };
            let folder = if on == imported.machine_id {
                Some(imported.folder.clone())
            } else {
                previews::checkout_for_worktree(store, &usable, &servers, &imported.project, on)
                    .await
                    .unwrap_or_else(|e| {
                        log::info!("import: no checkout for a worktree; node={on}: {e}");
                        None
                    })
            };
            let machine = usable.iter().find(|machine| machine.id == on);
            let (Some(folder), Some(machine)) = (folder, machine) else {
                log::info!("import: worktree skipped, no checkout online; node={on}");
                continue;
            };
            let Ok(shell) = machine.shell() else { continue };

            if let Err(why) =
                add_worktree(shell, &machine.name, &folder, branch, FALLBACK_DEFAULT).await
            {
                log::info!("import: worktree not added; node={on}: {why}");
            }
        }
    }

    /// Makes the project `request` asks for.
    pub async fn create(&self, request: &CreateProject) -> Result<CreatedProject> {
        let store = MeshStore::new(&self.mesh);
        let (credentials, machines) = (self.opened(), self.machines().await);
        let env = Env {
            store: &store,
            hosts: &self.hosts,
            effects: &MeshEffects,
            credentials: &credentials,
            machines: &machines,
        };
        Ok(flow::create_project(&env, request).await?)
    }

    /// The git hosts the user has a token for, and who a repository there can belong to.
    pub async fn forges(&self) -> Result<Vec<ProjectForge>> {
        Ok(project_forges(&self.hosts, &self.opened()).await?)
    }

    /// Whether importing `input` would clone the user's fork of it.
    pub async fn find_fork(&self, input: &str) -> Result<ImportFork> {
        let credentials = self.opened();
        let repo = RepoRef::parse(input, &forgejo_of(&credentials))?;
        let found = find_fork(&self.hosts, &credentials, &repo).await;
        Ok(ImportFork {
            project: repo.project(),
            fork: found.as_ref().map(|fork| fork.repo.project()),
            branches: found.map(|fork| fork.found.branches).unwrap_or_default(),
        })
    }

    /// Where `input` is checked out on the online machines, and its branches with work.
    pub async fn preview(&self, input: &str) -> Result<ImportPreview> {
        let usable: Vec<Machine> = self
            .machines()
            .await
            .into_iter()
            .filter(|machine| machine.shell.is_some())
            .collect();
        let servers = forgejo_of(&self.opened());
        let imports = MeshStore::new(&self.mesh).rows();
        Ok(previews::preview(&usable, &imports, &servers, input).await?)
    }

    /// The checkouts the machines' coding agents worked in that are not projects yet: `found`
    /// grouped by repository, leaving out what the mesh already has as a project.
    pub fn group_found(&self, found: Vec<MachineFound>) -> Vec<FoundProject> {
        let servers = forgejo_of(&self.opened());
        found::group(found, &MeshStore::new(&self.mesh).rows(), &servers)
    }

    /// The user's repositories per git host, newest update first, without those imported.
    pub async fn host_repos(&self) -> Result<Vec<HostRepos>> {
        let servers = forgejo_of(&self.opened());
        let (mut gh, mut tea) = (Vec::new(), Vec::new());

        for seen in self.seen().await {
            let (Some(shell), Some(env)) = (seen.machine.shell, seen.env) else {
                continue;
            };
            let has = |tool: &str| env.tools.iter().any(|known| known.name == tool);
            let signed_in = |tool: &str| {
                env.logins
                    .iter()
                    .any(|login| login.tool == tool && login.signed_in)
            };
            if has("gh") {
                gh.push((!signed_in("gh"), shell.clone()));
            }
            if has("teacli") {
                tea.push((!signed_in("teacli"), shell));
            }
        }

        let signed_in_first = |mut shells: Vec<(bool, Arc<dyn Shell>)>| {
            shells.sort_by_key(|(signed_out, _)| *signed_out);
            shells
                .into_iter()
                .map(|(_, shell)| shell)
                .collect::<Vec<_>>()
        };
        let mut hosts = read_hosts(&signed_in_first(gh), &signed_in_first(tea), &servers).await;
        let imported: Vec<String> = MeshStore::new(&self.mesh)
            .rows()
            .into_iter()
            .map(|row| row.project.to_ascii_lowercase())
            .collect();
        for host in &mut hosts {
            host.repos
                .retain(|repo| !imported.contains(&repo.name.to_ascii_lowercase()));
            host.repos.truncate(MAX_REPOS);
        }

        Ok(hosts)
    }
}
