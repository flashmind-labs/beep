//! Where a project's issues live: hosts and the machines that reach them.

use super::{
    super::{
        commands::{HostCommand, Shell},
        fleet::Fleet,
        model::{IssueHost, WorkerAgent},
    },
    IssueError, host_failed, run_host,
};
use crate::{account::user::User, store::ProjectRow};
use anyhow::{Result, anyhow};
use iroh::EndpointId;
use std::sync::Arc;
use url::Url;

/// Where a project's open issues are read: its host, via a shell on a machine with the project.
pub(crate) struct IssueSource {
    pub(crate) host: IssueHost,
    pub(crate) machine: EndpointId,
    pub(crate) shell: Arc<dyn Shell>,
}

/// How the user asked a tracked issue's worker to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IssueWork {
    pub(crate) agent: WorkerAgent,
    /// `None` picks a machine at start.
    pub(crate) machine: Option<EndpointId>,
}

/// The issues host a repository web page is on.
pub(super) fn host_of_url(url: &str) -> Option<IssueHost> {
    IssueHost::of_hostname(Url::parse(url).ok()?.host_str()?)
}

/// A project's issues host and the machines that can reach it.
pub(crate) struct HostAndMachines {
    pub(crate) host: IssueHost,
    /// The home checkout's machine, else the machines it was imported on.
    pub(crate) machines: Vec<EndpointId>,
    /// Whether the machines are the home checkout's.
    pub(super) home: bool,
}

/// From the home checkout's issues role and machine, else the imported web page's host and
/// machines. `None` without a GitHub or Forgejo host.
pub(crate) async fn host_and_machines(
    user: &User,
    project: &ProjectRow,
) -> Result<Option<HostAndMachines>, IssueError> {
    let checkouts = user.checkouts(project.id).await?;
    if let Some(home) = checkouts.iter().find(|checkout| checkout.home) {
        return Ok(project.roles.issues.clone().map(|host| HostAndMachines {
            host,
            machines: vec![home.machine],
            home: true,
        }));
    }
    let name = project.name.to_string();
    let imports: Vec<_> = user
        .imports()
        .await?
        .into_iter()
        .filter(|import| import.project.eq_ignore_ascii_case(&name))
        .collect();
    Ok(imports
        .iter()
        .find_map(|import| host_of_url(&import.url))
        .map(|host| HostAndMachines {
            host,
            machines: imports
                .iter()
                .filter_map(|import| import.endpoint.parse::<EndpointId>().ok())
                .collect(),
            home: false,
        }))
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
/// `None` without a GitHub or Forgejo host; refused when none is online.
pub(super) async fn issue_sources(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
) -> Result<Option<Vec<IssueSource>>, IssueError> {
    let Some(found) = host_and_machines(user, project).await? else {
        return Ok(None);
    };
    let mut seen = Vec::new();
    let sources: Vec<IssueSource> = found
        .machines
        .iter()
        .filter(|machine| {
            let new = !seen.contains(*machine);
            seen.push(**machine);
            new
        })
        .filter_map(|machine| {
            fleet.shell(machine).map(|shell| IssueSource {
                host: found.host.clone(),
                machine: *machine,
                shell,
            })
        })
        .collect();
    if sources.is_empty() {
        return Err(IssueError::Refused(if found.home {
            "The home checkout's machine is offline or offers no shell.".to_owned()
        } else {
            "No machine the project is on is online.".to_owned()
        }));
    }
    Ok(Some(sources))
}

/// Tries `command` on each source in turn so one broken CLI does not hide the list.
/// Gives the source that answered; when all fail, the last error and the machines tried.
pub(super) async fn read_any<'s, T>(
    fleet: &dyn Fleet,
    sources: &'s [IssueSource],
    project: &ProjectRow,
    command: HostCommand<'_>,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&'s IssueSource, T), IssueError> {
    let mut last = None;
    for source in sources {
        let read = run_host(source.shell.as_ref(), &source.host, project, command)
            .await
            .and_then(|out| parse(&out));
        match read {
            Ok(value) => return Ok((source, value)),
            Err(error) => {
                log::info!("projects: reading the host failed on one machine: {error:#}");
                last = Some(error);
            }
        }
    }
    let error = last.unwrap_or_else(|| anyhow!("no machine to read it on"));
    if sources.len() < 2 {
        return Err(host_failed(&error));
    }
    // Names only make the message clearer: without them it still says why.
    let machines = fleet.machines().await.unwrap_or_else(|names_error| {
        log::info!("projects: the machines' names are unknown: {names_error:#}");
        Vec::new()
    });
    let tried: Vec<String> = sources
        .iter()
        .map(|source| {
            machines
                .iter()
                .find(|machine| machine.id == source.machine)
                .map_or_else(|| "a machine".to_owned(), |machine| machine.name.clone())
        })
        .collect();
    Err(IssueError::Refused(format!(
        "Could not read the issues host on {}: {error:#}",
        tried.join(", ")
    )))
}
