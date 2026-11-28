//! A project's issues and pull requests, read from its git host through a machine that has the
//! project. No server is involved: the desktop asks the machine's own `gh` or `teacli`.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    nearest_first,
    records::{now_ms, projects, time},
};
use async_trait::async_trait;
use beep_mesh::Mesh;
use beep_projects::issues::{
    HostGates, ImportedPlace, MachineName, MachineView, Machines, ProjectStore, host_of_url,
    issue_view, list_open, list_open_prs, machine_view, pull_request_head, pull_request_view,
};
use beep_projects::model::plain_path;
use beep_projects::{
    Shell,
    imports::machine::Machine,
    model::{IssueHost, RepoName},
};
use beep_proto::{
    GitItemKind, LabeledProjectLists, ProjectIssue, ProjectLists, ProjectPullRequest,
    PullRequestHead, ThreadDetail,
};
use iroh::EndpointId;
use std::str::FromStr;
use std::{collections::HashMap, future::Future, sync::Arc};
use uuid::Uuid;

/// A project's id on this desktop. The apps address a project by an id, and with no server to
/// make one it comes from the name, the same on every desktop.
pub fn project_id(name: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("beep:project:{name}").as_bytes(),
    )
}

impl Desk {
    /// The name of the project `id` is the id of, when this mesh has it.
    pub fn project_name(&self, id: Uuid) -> Option<String> {
        crate::records::projects(&self.mesh)
            .into_iter()
            .find_map(|stored| (project_id(&stored.name) == id).then_some(stored.name))
    }

    /// What reading `name`'s host needs, and whether only this Mac is asked. With `local_only`,
    /// a checkout of the project on this Mac, and no other machine known to answer for the host
    /// instead, only this Mac is asked, so nothing waits on another machine. Otherwise every
    /// machine with a repository on the host is asked, nearest first.
    pub(crate) async fn host_reads(
        &self,
        name: &str,
        local_only: bool,
    ) -> Result<(DeskProject, DeskMachines, bool)> {
        let store = DeskProject::of(&self.mesh, name)?;
        let gates = &self.hosts.gates;
        let local_answers = store.host.as_ref().is_none_or(|host| {
            !gates.is_paused(self.local, host)
                && gates
                    .preferred(host)
                    .is_none_or(|machine| machine == self.local)
        });

        if local_only
            && local_answers
            && store.is_on(self.local)
            && let Some(local) = self.local_machine().await
        {
            return Ok((store, DeskMachines::of(&[local], gates), true));
        }

        let machines: Vec<Machine> = self
            .seen_among(&store.machine_ids())
            .await
            .into_iter()
            .map(|seen| seen.machine)
            .collect();
        Ok((store, DeskMachines::of(&machines, gates), false))
    }

    /// Runs `read` against what reading the project's host needs, asking this Mac alone when the
    /// project is checked out here, and the others only if that failed.
    async fn host_first<T, R, F>(&self, project: &str, read: R) -> Result<T>
    where
        R: Fn(DeskProject, DeskMachines) -> F,
        F: Future<Output = Result<T>>,
    {
        let (store, machines, alone) = self.host_reads(project, true).await?;

        match read(store, machines).await {
            Err(_) if alone => {
                let (store, machines, _) = self.host_reads(project, false).await?;
                read(store, machines).await
            }
            done => done,
        }
    }

    /// The project's open issues no task tracks.
    pub async fn issues(&self, project: &str) -> Result<Vec<ProjectIssue>> {
        self.host_first(project, |store, machines| async move {
            let open = list_open(&store, &machines).await?;
            Ok(open.into_iter().map(issue_view).collect())
        })
        .await
    }

    /// Every open pull request of the project, newest first.
    pub async fn pull_requests(&self, project: &str) -> Result<Vec<ProjectPullRequest>> {
        self.host_first(project, |store, machines| async move {
            let listed = list_open_prs(&store, &machines).await?;
            Ok(listed.into_iter().map(pull_request_view).collect())
        })
        .await
    }

    /// The branch pull request `number` is made from.
    pub async fn pr_head(&self, project: &str, number: u64) -> Result<PullRequestHead> {
        self.host_first(project, |store, machines| async move {
            Ok(pull_request_head(&store, &machines, number).await?)
        })
        .await
    }

    /// Pull request or issue `number` read in full: its text, comments, review and checks.
    pub async fn thread_detail(&self, project: &str, number: u64) -> Result<ThreadDetail> {
        self.host_first(project, |store, machines| async move {
            Ok(beep_projects::thread_detail::read(&store, &machines, number).await?)
        })
        .await
    }

    /// Posts `body` as a comment on pull request or issue `number`, asking this Mac when the
    /// project is checked out here, else another machine that has it. Never posted twice.
    pub async fn comment_thread(
        &self,
        project: &str,
        kind: GitItemKind,
        number: u64,
        body: &str,
    ) -> Result<()> {
        let (store, machines, _) = self.host_reads(project, true).await?;
        Ok(beep_projects::thread_detail::comment(&store, &machines, kind, number, body).await?)
    }

    /// The project's open pull requests, with their labels, and its issues no task tracks. A list
    /// that cannot be read says why beside the other.
    pub async fn lists(&self, project: &str) -> Result<LabeledProjectLists> {
        let (local, alone) = self.lists_on(project, true).await?;
        let failed = local.lists.prs_error.is_some() || local.lists.issues_error.is_some();

        if failed && alone {
            // This Mac could not read it; the others may.
            return Ok(self.lists_on(project, false).await?.0);
        }

        Ok(local)
    }

    /// The lists read through the machines `host_reads` gives, and whether only this Mac was
    /// asked.
    async fn lists_on(
        &self,
        project: &str,
        local_only: bool,
    ) -> Result<(LabeledProjectLists, bool)> {
        let (store, machines, alone) = self.host_reads(project, local_only).await?;
        let (prs, issues) = tokio::join!(
            list_open_prs(&store, &machines),
            list_open(&store, &machines),
        );
        let (mut pull_requests, mut pr_labels) = (Vec::new(), Vec::new());
        let prs_error = match prs {
            Ok(listed) => {
                for one in listed {
                    pr_labels.push(one.pr.labels.clone());
                    pull_requests.push(pull_request_view(one));
                }

                None
            }
            Err(e) => Some(e.to_string()),
        };
        let (issues, issues_error) = match issues {
            Ok(open) => (open.into_iter().map(issue_view).collect(), None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let lists = LabeledProjectLists {
            lists: ProjectLists {
                pull_requests,
                prs_error,
                issues,
                issues_error,
                synced_at: Some(time(now_ms())),
            },
            pr_labels,
        };
        Ok((lists, alone))
    }

    /// What the machine that holds `project` says about its account and the repository, asking
    /// this Mac alone when the project is checked out here, and the others only if that failed.
    pub(crate) async fn machine_view(&self, project: &str) -> Option<MachineView> {
        let (store, machines, alone) = self.host_reads(project, true).await.ok()?;

        match machine_view(&store, &machines).await {
            None if alone => {
                let (store, machines, _) = self.host_reads(project, false).await.ok()?;
                machine_view(&store, &machines).await
            }
            view => view,
        }
    }
}

/// The names of the projects the mesh records.
pub fn project_names(mesh: &beep_mesh::Mesh) -> Vec<String> {
    crate::records::projects(mesh)
        .into_iter()
        .map(|stored| stored.name)
        .collect()
}

/// Whether the mesh records `project`, which then has no server to ask about its places.
pub fn has_project(mesh: &beep_mesh::Mesh, project: &str) -> bool {
    crate::records::find_project(mesh, project).is_some()
}

/// Where `project` is checked out, by machine and folder, as the mesh records it.
pub fn places(mesh: &beep_mesh::Mesh, project: &str) -> Vec<(iroh::EndpointId, String)> {
    crate::records::find_project(mesh, project)
        .map(|stored| {
            stored
                .places
                .into_iter()
                .map(|(machine, place)| (machine, plain_path(&place.folder).into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// One project, for reading its issues host: its checkouts nearest first, then the other
/// machines with a repository on the same host, the order they are asked in. The host's CLI
/// takes the repository by name, so any machine signed in to the host can read it.
pub(crate) struct DeskProject {
    name: RepoName,
    host: Option<IssueHost>,
    places: Vec<ImportedPlace>,
    /// How many of `places` are the project's own checkouts.
    own: usize,
}

impl DeskProject {
    pub(crate) fn of(mesh: &Mesh, name: &str) -> Result<Self> {
        let name = RepoName::from_str(name).map_err(|e| FleetError::refused(e.to_string()))?;
        let all = projects(mesh);
        let stored = all
            .iter()
            .find(|stored| stored.name.eq_ignore_ascii_case(&name.to_string()))
            .ok_or(FleetError::NotFound)?;
        let host = host_of_url(&stored.record.url);
        let mut own: Vec<EndpointId> = stored.places.keys().copied().collect();
        nearest_first(mesh, &mut own, |machine| *machine);
        let mut others: Vec<EndpointId> = Vec::new();

        if host.is_some() {
            for machine in all
                .iter()
                .filter(|other| host_of_url(&other.record.url) == host)
                .flat_map(|other| other.places.keys())
            {
                if !own.contains(machine) && !others.contains(machine) {
                    others.push(*machine);
                }
            }

            nearest_first(mesh, &mut others, |machine| *machine);
        }

        let places = own
            .iter()
            .chain(&others)
            .map(|machine| ImportedPlace {
                url: stored.record.url.clone(),
                machine: Some(*machine),
            })
            .collect();
        Ok(Self {
            name,
            host,
            places,
            own: own.len(),
        })
    }

    /// The machines that may read the project's host.
    pub(crate) fn machine_ids(&self) -> Vec<EndpointId> {
        self.places
            .iter()
            .filter_map(|place| place.machine)
            .collect()
    }

    /// Whether the project has a checkout on `machine`.
    pub(crate) fn is_on(&self, machine: EndpointId) -> bool {
        self.places[..self.own]
            .iter()
            .any(|place| place.machine == Some(machine))
    }
}

#[async_trait]
impl ProjectStore for DeskProject {
    fn name(&self) -> &RepoName {
        &self.name
    }

    async fn imports(&self) -> anyhow::Result<Vec<ImportedPlace>> {
        Ok(self.places.clone())
    }
}

/// The online machines' shells, for reading a host through one of them.
pub(crate) struct DeskMachines {
    shells: HashMap<EndpointId, Arc<dyn Shell>>,
    names: Vec<MachineName>,
    gates: Arc<HostGates>,
}

impl DeskMachines {
    pub(crate) fn of(machines: &[Machine], gates: &Arc<HostGates>) -> Self {
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
        Self {
            shells,
            names,
            gates: gates.clone(),
        }
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

    fn gates(&self) -> Option<&HostGates> {
        Some(&self.gates)
    }
}
