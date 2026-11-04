//! Where a project's issues live: hosts and the machines that reach them.

use super::{
    super::{
        commands::HostCommand,
        fleet::Fleet,
        model::{IssueHost, RepoName, WorkerAgent},
    },
    IssueError,
};
use crate::{account::user::User, store::ProjectRow};
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::issues::{self, ImportedPlace, MachineName, ProjectStore};
use iroh::EndpointId;

pub(crate) use beep_projects::issues::{HostAndMachines, IssueSource};

/// How the user asked a tracked issue's worker to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IssueWork {
    pub(crate) agent: WorkerAgent,
    /// `None` picks a machine at start.
    pub(crate) machine: Option<EndpointId>,
}

/// A project as the issue code reads it from Postgres.
struct StoredProject<'a> {
    user: &'a User,
    project: &'a ProjectRow,
}

#[async_trait]
impl ProjectStore for StoredProject<'_> {
    fn name(&self) -> &RepoName {
        &self.project.name
    }

    fn issues_role(&self) -> Option<&IssueHost> {
        self.project.roles.issues.as_ref()
    }

    async fn home_machine(&self) -> Result<Option<EndpointId>> {
        let checkouts = self.user.checkouts(self.project.id).await?;
        Ok(checkouts
            .iter()
            .find(|checkout| checkout.home)
            .map(|checkout| checkout.machine))
    }

    async fn imports(&self) -> Result<Vec<ImportedPlace>> {
        let name = self.project.name.to_string();
        Ok(self
            .user
            .imports()
            .await?
            .into_iter()
            .filter(|import| import.project.eq_ignore_ascii_case(&name))
            .filter_map(|import| {
                Some(ImportedPlace {
                    machine: import.endpoint.parse().ok()?,
                    url: import.url,
                })
            })
            .collect())
    }
}

/// A project's issues host and the machines that can reach it.
pub(crate) async fn host_and_machines(
    user: &User,
    project: &ProjectRow,
) -> Result<Option<HostAndMachines>, IssueError> {
    Ok(issues::host_and_machines(&StoredProject { user, project }).await?)
}

/// `project`'s issues host, if it has one.
pub(crate) async fn issue_host(
    user: &User,
    project: &ProjectRow,
) -> Result<Option<IssueHost>, IssueError> {
    Ok(host_and_machines(user, project)
        .await?
        .map(|found| found.host))
}

/// [`issue_host`] with a shell on an online machine. `None` without a GitHub or Forgejo host.
pub(crate) async fn issue_source(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Option<IssueSource>, IssueError> {
    Ok(issue_sources(fleet, user, project)
        .await?
        .and_then(|sources| sources.into_iter().next()))
}

/// Every online place to read `project`'s issues: the home machine, else import machines by id.
pub(super) async fn issue_sources(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Option<Vec<IssueSource>>, IssueError> {
    Ok(
        issues::issue_sources(&StoredProject { user, project }, |machine| {
            fleet.shell(machine)
        })
        .await?,
    )
}

/// Tries `command` on each source in turn so one broken CLI does not hide the list.
pub(super) async fn read_any<'s, T>(
    fleet: &dyn Fleet,
    sources: &'s [IssueSource],
    project: &ProjectRow,
    command: HostCommand<'_>,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&'s IssueSource, T), IssueError> {
    let names = async || {
        // Names only make the message clearer: without them it still says why.
        let machines = fleet.machines().await.unwrap_or_else(|error| {
            log::info!("projects: the machines' names are unknown: {error:#}");
            Vec::new()
        });
        machines
            .into_iter()
            .map(|machine| MachineName {
                id: machine.id,
                name: machine.name,
            })
            .collect()
    };
    Ok(issues::read_any(sources, &project.name, command, parse, names).await?)
}
