//! A project's issues and pull requests, read from its git host through a machine that has the
//! project. No server is involved: the desktop asks the machine's own `gh` or `teacli`.

use crate::{
    desk::Desk,
    error::Result,
    projects::{DeskMachines, DeskProject},
    records::{now_ms, time},
};
use beep_projects::issues::{
    MachineView, issue_view, list_open, list_open_prs, machine_view, pull_request_head,
    pull_request_view,
};
use beep_projects::model::plain_path;
use beep_proto::{
    LabeledProjectLists, ProjectIssue, ProjectLists, ProjectPullRequest, PullRequestHead,
};
use std::future::Future;
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

    /// What reading `name`'s host needs. With `local_only`, and a checkout of the project on this
    /// Mac, only this Mac is asked, so nothing waits on another machine. Otherwise the machines
    /// the project is on are asked, and no other.
    pub(crate) async fn host_reads(
        &self,
        name: &str,
        local_only: bool,
    ) -> Result<(DeskProject, DeskMachines)> {
        let store = DeskProject::of(&self.mesh, name)?;
        if local_only
            && store.is_on(self.local)
            && let Some(local) = self.local_machine().await
        {
            return Ok((store, DeskMachines::of(&[local])));
        }
        let machines = self.machines_among(&store.machine_ids()).await;
        Ok((store, DeskMachines::of(&machines)))
    }

    /// Runs `read` against what reading the project's host needs, asking this Mac alone when the
    /// project is checked out here, and the others only if that failed.
    async fn host_first<T, R, F>(&self, project: &str, read: R) -> Result<T>
    where
        R: Fn(DeskProject, DeskMachines) -> F,
        F: Future<Output = Result<T>>,
    {
        let (store, machines) = self.host_reads(project, true).await?;
        let here = store.is_on(self.local);
        match read(store, machines).await {
            Err(_) if here => {
                let (store, machines) = self.host_reads(project, false).await?;
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
            let head = pull_request_head(&store, &machines, number).await?;
            Ok(PullRequestHead {
                number: head.number,
                branch: head.branch,
                fork: head.fork,
            })
        })
        .await
    }

    /// The project's open pull requests, with their labels, and its issues no task tracks. A list
    /// that cannot be read says why beside the other.
    pub async fn lists(&self, project: &str) -> Result<LabeledProjectLists> {
        let local = self.lists_on(project, true).await?;
        let failed = local.lists.prs_error.is_some() || local.lists.issues_error.is_some();
        if failed && DeskProject::of(&self.mesh, project)?.is_on(self.local) {
            // This Mac could not read it; the others may.
            return self.lists_on(project, false).await;
        }
        Ok(local)
    }

    async fn lists_on(&self, project: &str, local_only: bool) -> Result<LabeledProjectLists> {
        let (store, machines) = self.host_reads(project, local_only).await?;
        let (prs, issues) = tokio::join!(
            beep_projects::issues::list_open_prs(&store, &machines),
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
        Ok(LabeledProjectLists {
            lists: ProjectLists {
                pull_requests,
                prs_error,
                issues,
                issues_error,
                synced_at: Some(time(now_ms())),
            },
            pr_labels,
        })
    }

    /// What the machine that holds `project` says about its account and the repository, asking
    /// this Mac alone when the project is checked out here, and the others only if that failed.
    pub(crate) async fn machine_view(&self, project: &str) -> Option<MachineView> {
        let (store, machines) = self.host_reads(project, true).await.ok()?;
        let here = store.is_on(self.local);
        match machine_view(&store, &machines).await {
            None if here => {
                let (store, machines) = self.host_reads(project, false).await.ok()?;
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
