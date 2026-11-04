//! A project's issues and pull requests, read from its git host through a machine that has the
//! project. No server is involved: the desktop asks the machine's own `gh` or `teacli`.

use crate::{
    desk::Desk,
    error::Result,
    projects::{DeskMachines, DeskProject},
    records::scan,
};
use beep_mesh::Key;
use beep_projects::issues::{
    issue_view, list_open, list_open_prs, pull_request_head, pull_request_view,
};
use beep_proto::{ProjectIssue, ProjectPullRequest, PullRequestHead};
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
        scan::<crate::records::ProjectRecord>(&self.mesh, "project/")
            .into_iter()
            .find_map(|(key, _)| match key {
                Key::Project(name) if project_id(&name) == id => Some(name),
                _ => None,
            })
    }

    async fn host_reads(&self, name: &str) -> Result<(DeskProject, DeskMachines)> {
        let store = DeskProject::of(&self.mesh, name)?;
        Ok((store, DeskMachines::of(&self.machines().await)))
    }

    /// The project's open issues no task tracks.
    pub async fn issues(&self, project: &str) -> Result<Vec<ProjectIssue>> {
        let (store, machines) = self.host_reads(project).await?;
        let open = list_open(&store, &machines).await?;
        Ok(open.into_iter().map(issue_view).collect())
    }

    /// Every open pull request of the project, newest first.
    pub async fn pull_requests(&self, project: &str) -> Result<Vec<ProjectPullRequest>> {
        let (store, machines) = self.host_reads(project).await?;
        let listed = list_open_prs(&store, &machines).await?;
        Ok(listed.into_iter().map(pull_request_view).collect())
    }

    /// The branch pull request `number` is made from.
    pub async fn pr_head(&self, project: &str, number: u64) -> Result<PullRequestHead> {
        let (store, machines) = self.host_reads(project).await?;
        let head = pull_request_head(&store, &machines, number).await?;
        Ok(PullRequestHead {
            number: head.number,
            branch: head.branch,
            fork: head.fork,
        })
    }
}
