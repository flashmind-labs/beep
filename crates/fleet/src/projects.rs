//! Projects on the desktop: the records in the mesh, and importing, creating, previewing and
//! removing them through a shell on each machine. The code that decides what to run is the
//! server's (`beep_projects`); this is the store and the machines it runs against.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{
        GroupRecord, PinRecord, Place, ProjectRecord, Stored, WRITES, at, find_project, now_ms,
        place_key, project_key, projects, read, scan, write,
    },
};
use async_trait::async_trait;
use beep_mesh::{Key, Mesh};
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
        records::{ImportRow, NewImport},
        repo::RepoRef,
        store::ImportStore,
    },
    issues::{ImportedPlace, MachineName, Machines, ProjectStore},
    model::{IssueHost, RepoName, branch_name},
};
use beep_proto::{
    BranchPlace, CheckoutFailure, CreateProject, CreatedProject, FoundProject, HostRepos,
    ImportFork, ImportPreview, ImportedProject, ProjectForge, ProjectGroup,
    groups::{group_name, group_paths},
    node::mesh_state::SyncSpec,
};
use iroh::EndpointId;
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

/// The most worktrees one import adds.
const MAX_IMPORT_WORKTREES: usize = 20;
/// The branch worktrees are cut from when the checkout says none.
const FALLBACK_DEFAULT: &str = "main";
/// The most repositories one host lists.
const MAX_REPOS: usize = 100;

fn row(stored: &Stored, machine: EndpointId, place: &Place) -> ImportRow {
    ImportRow {
        project: stored.name.clone(),
        endpoint: machine.to_string(),
        folder: place.folder.clone(),
        url: stored.record.url.clone(),
        imported_at: at(place.imported_ms),
        fork: place
            .fork
            .as_deref()
            .and_then(|fork| RepoName::parse(fork).ok()),
    }
}

/// The projects as the import code reads and writes them: `project/<name>` and one
/// `project/<name>/<machine>` per checkout in the mesh.
pub(crate) struct MeshStore {
    mesh: Mesh,
}

impl MeshStore {
    pub(crate) fn new(mesh: &Mesh) -> Self {
        Self { mesh: mesh.clone() }
    }

    pub(crate) fn rows(&self) -> Vec<ImportRow> {
        projects(&self.mesh)
            .iter()
            .flat_map(|stored| {
                stored
                    .places
                    .iter()
                    .map(|(machine, place)| row(stored, *machine, place))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Makes the project's own key if it is not there, with `url`.
    async fn ensure(&self, name: &str, url: &str) -> anyhow::Result<()> {
        let key = project_key(name);
        let mut record = read::<ProjectRecord>(&self.mesh, &key).unwrap_or_default();
        if record.url.is_empty() {
            record.url = url.to_owned();
            write(&self.mesh, key, Some(&record)).await?;
        }
        Ok(())
    }

    /// The name the project is stored under, which is the one it was first imported as, in any
    /// case it is asked for; the name given when it is new.
    fn name_of(&self, project: &str) -> String {
        find_project(&self.mesh, project).map_or_else(|| project.to_owned(), |stored| stored.name)
    }

    fn stored_row(&self, name: &str, machine: EndpointId) -> anyhow::Result<ImportRow> {
        let stored = find_project(&self.mesh, name).ok_or_else(|| anyhow::anyhow!("not stored"))?;
        let place = stored
            .places
            .get(&machine)
            .ok_or_else(|| anyhow::anyhow!("not stored"))?;
        Ok(row(&stored, machine, place))
    }
}

#[async_trait]
impl ImportStore for MeshStore {
    async fn imports(&self) -> anyhow::Result<Vec<ImportRow>> {
        Ok(self.rows())
    }

    async fn save_import(&self, import: NewImport<'_>) -> anyhow::Result<ImportRow> {
        let machine = import.endpoint.parse::<EndpointId>()?;
        let _write = WRITES.lock().await;
        let name = self.name_of(import.project);
        self.ensure(&name, import.url).await?;
        let key = place_key(&name, machine);
        let place = Place {
            folder: import.folder.to_owned(),
            // Moving a checkout keeps when it was first imported.
            imported_ms: read::<Place>(&self.mesh, &key).map_or_else(now_ms, |old| old.imported_ms),
            fork: read::<Place>(&self.mesh, &key).and_then(|old| old.fork),
        };
        write(&self.mesh, key, Some(&place)).await?;
        self.stored_row(&name, machine)
    }

    async fn link_import(&self, import: NewImport<'_>) -> anyhow::Result<bool> {
        let machine = import.endpoint.parse::<EndpointId>()?;
        let _write = WRITES.lock().await;
        let name = self.name_of(import.project);
        let key = place_key(&name, machine);
        if self.mesh.get(&key).is_some() {
            return Ok(false);
        }
        self.ensure(&name, import.url).await?;
        let place = Place {
            folder: import.folder.to_owned(),
            imported_ms: now_ms(),
            fork: None,
        };
        write(&self.mesh, key, Some(&place)).await?;
        Ok(true)
    }

    async fn set_import_fork(
        &self,
        project: &str,
        endpoint: &str,
        fork: Option<&RepoName>,
    ) -> anyhow::Result<()> {
        let machine = endpoint.parse::<EndpointId>()?;
        let _write = WRITES.lock().await;
        let key = place_key(&self.name_of(project), machine);
        let Some(mut place) = read::<Place>(&self.mesh, &key) else {
            return Ok(());
        };
        let fork = fork.map(ToString::to_string);
        if place.fork == fork {
            return Ok(());
        }
        place.fork = fork;
        write(&self.mesh, key, Some(&place)).await?;
        Ok(())
    }
}

/// One project, for reading its issues host.
pub(crate) struct DeskProject {
    name: RepoName,
    places: Vec<ImportedPlace>,
}

impl DeskProject {
    pub(crate) fn of(mesh: &Mesh, name: &str) -> Result<Self> {
        let name = RepoName::parse(name).map_err(|e| FleetError::refused(e.to_string()))?;
        let stored = find_project(mesh, &name.to_string()).ok_or(FleetError::NotFound)?;
        let places = stored
            .places
            .keys()
            .map(|machine| ImportedPlace {
                url: stored.record.url.clone(),
                machine: Some(*machine),
            })
            .collect();
        Ok(Self { name, places })
    }
}

impl DeskProject {
    /// Whether the project has a checkout on `machine`.
    /// The machines the project is checked out on.
    pub(crate) fn machine_ids(&self) -> Vec<EndpointId> {
        self.places
            .iter()
            .filter_map(|place| place.machine)
            .collect()
    }

    pub(crate) fn is_on(&self, machine: EndpointId) -> bool {
        self.places
            .iter()
            .any(|place| place.machine == Some(machine))
    }
}

#[async_trait]
impl ProjectStore for DeskProject {
    fn name(&self) -> &RepoName {
        &self.name
    }

    /// The desktop has no project agent to set a home checkout up, so the host is read from the
    /// page the project was imported from.
    fn issues_role(&self) -> Option<&IssueHost> {
        None
    }

    async fn home_machine(&self) -> anyhow::Result<Option<EndpointId>> {
        Ok(None)
    }

    async fn imports(&self) -> anyhow::Result<Vec<ImportedPlace>> {
        Ok(self.places.clone())
    }

    /// Tasks that track issues belong to the project agent, which is on the server.
    async fn tracked_issues(&self, _: &IssueHost) -> anyhow::Result<Vec<u64>> {
        Ok(Vec::new())
    }
}

/// The online machines' shells, for reading a host through one of them.
pub(crate) struct DeskMachines {
    shells: HashMap<EndpointId, Arc<dyn Shell>>,
    names: Vec<MachineName>,
}

impl DeskMachines {
    pub(crate) fn of(machines: &[Machine]) -> Self {
        let mut shells = HashMap::new();
        let mut names = Vec::new();
        for machine in machines {
            let Ok(id) = machine.id.parse::<EndpointId>() else {
                continue;
            };
            names.push(MachineName {
                id,
                name: machine.name.clone(),
            });
            if let Some(shell) = &machine.shell {
                shells.insert(id, shell.clone());
            }
        }
        Self { shells, names }
    }
}

#[async_trait]
impl Machines for DeskMachines {
    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>> {
        self.shells.get(machine).cloned()
    }

    async fn names(&self) -> Vec<MachineName> {
        self.names.clone()
    }
}

/// After an import: a new project's checkouts sync with each other, by `sync/<name>` in the
/// mesh, which each node reads.
struct MeshEffects {
    mesh: Mesh,
}

#[async_trait]
impl Effects for MeshEffects {
    /// The desktop does not look through the other machines on its own; the Add a project sheet
    /// finds checkouts there when it is opened.
    async fn imported(&self, _: &str) {}

    async fn join_sync(
        &self,
        imported: &[ImportedProject],
        _: &mut Vec<CheckoutFailure>,
    ) -> anyhow::Result<()> {
        let Some(first) = imported.first() else {
            return Ok(());
        };
        if imported.len() < 2 {
            return Ok(());
        }
        let _write = WRITES.lock().await;
        let key = Key::Sync(first.project.clone());
        let mut spec = read::<SyncSpec>(&self.mesh, &key).unwrap_or_default();
        for landed in imported {
            spec.folders
                .insert(landed.machine_id.parse()?, landed.folder.clone());
        }
        write(&self.mesh, key, Some(&spec)).await?;
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
            .devices()
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
        let (store, effects) = (MeshStore::new(&self.mesh), self.effects());
        let (credentials, machines) = (self.opened(), self.machines().await);
        let env = Env {
            store: &store,
            hosts: &self.hosts,
            effects: &effects,
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

    fn effects(&self) -> MeshEffects {
        MeshEffects {
            mesh: self.mesh.clone(),
        }
    }

    /// Makes the project `request` asks for.
    pub async fn create(&self, request: &CreateProject) -> Result<CreatedProject> {
        let (store, effects) = (MeshStore::new(&self.mesh), self.effects());
        let (credentials, machines) = (self.opened(), self.machines().await);
        let env = Env {
            store: &store,
            hosts: &self.hosts,
            effects: &effects,
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
            if env.os == "windows" {
                continue;
            }
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

    /// Forgets a project: its checkouts stay on the machines, and so do their files.
    pub async fn remove(&self, name: &str) -> Result<()> {
        let _write = WRITES.lock().await;
        let name = name.trim();
        let stored = find_project(&self.mesh, name).ok_or(FleetError::NotFound)?;
        for machine in stored.places.keys() {
            self.mesh
                .set(place_key(&stored.name, *machine), None)
                .await?;
        }
        write::<ProjectRecord>(&self.mesh, project_key(&stored.name), None).await?;
        for key in [Key::Sync(stored.name.clone()), Key::Pin(stored.name)] {
            if self.mesh.get(&key).is_some() {
                self.mesh.set(key, None).await?;
            }
        }
        Ok(())
    }

    /// Whether the mesh has the project `name`.
    pub fn has_project(&self, name: &str) -> bool {
        find_project(&self.mesh, name).is_some()
    }

    /// Whether the mesh has the group `id`.
    pub fn has_group(&self, id: Uuid) -> bool {
        self.mesh.get(&Key::Group(id.to_string())).is_some()
    }

    /// The projects pinned to the top of the sidebar, oldest pin first.
    pub fn on_top(&self) -> Vec<String> {
        let mut pins: Vec<(u64, String)> = scan::<PinRecord>(&self.mesh, "pin/")
            .into_iter()
            .filter_map(|(key, pin)| match key {
                Key::Pin(name) => Some((pin.at_ms, name)),
                _ => None,
            })
            .collect();
        pins.sort();
        pins.into_iter().map(|(_, name)| name).collect()
    }

    /// Pins `project` to the top of the sidebar, or unpins it.
    pub async fn pin(&self, project: &str, pinned: bool) -> Result<()> {
        let _write = WRITES.lock().await;
        let key = Key::Pin(project.to_owned());
        if pinned {
            if read::<PinRecord>(&self.mesh, &key).is_none() {
                write(&self.mesh, key, Some(&PinRecord { at_ms: now_ms() })).await?;
            }
        } else if self.mesh.get(&key).is_some() {
            self.mesh.set(key, None).await?;
        }
        Ok(())
    }

    /// The user's groups, oldest first.
    pub fn groups(&self) -> Vec<ProjectGroup> {
        let mut groups: Vec<(u64, ProjectGroup)> = scan::<GroupRecord>(&self.mesh, "group/")
            .into_iter()
            .filter_map(|(key, record)| {
                let Key::Group(id) = key else { return None };
                Some((
                    record.created_ms,
                    ProjectGroup {
                        id: id.parse().ok()?,
                        name: record.name,
                        paths: record.paths,
                    },
                ))
            })
            .collect();
        groups.sort_by_key(|(created, _)| *created);
        groups.into_iter().map(|(_, group)| group).collect()
    }

    /// Makes a group without `id`, else changes the group `id`.
    pub async fn save_group(
        &self,
        id: Option<Uuid>,
        name: &str,
        paths: &[String],
    ) -> Result<ProjectGroup> {
        let paths = group_paths(paths)?;
        let name = group_name(name, &paths)?;
        let _write = WRITES.lock().await;
        let id = id.unwrap_or_else(Uuid::new_v4);
        let key = Key::Group(id.to_string());
        let created_ms = match (id, read::<GroupRecord>(&self.mesh, &key)) {
            (_, Some(old)) => old.created_ms,
            _ => now_ms(),
        };
        let record = GroupRecord {
            name: name.clone(),
            paths: paths.clone(),
            created_ms,
        };
        write(&self.mesh, key, Some(&record)).await?;
        Ok(ProjectGroup { id, name, paths })
    }

    pub async fn delete_group(&self, id: Uuid) -> Result<()> {
        let _write = WRITES.lock().await;
        let key = Key::Group(id.to_string());
        if self.mesh.get(&key).is_none() {
            return Err(FleetError::NotFound);
        }
        self.mesh.set(key, None).await?;
        Ok(())
    }
}
