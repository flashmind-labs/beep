//! Reading a project's issues host through its machines: which machines can reach it, and trying
//! each in turn.

mod host;

pub use host::{clipped, head, host_failed, read_any, read_first, run_host, run_rendered};
use host::{failed_everywhere, is_not_found, read_sources, set_up_on};

use crate::{
    Shell,
    commands::{
        HostCommand, OpenIssue, OpenPr, PrHead, parse_can_push, parse_login, parse_open_issues,
        parse_open_prs, parse_pr_head,
    },
    model::{IssueHost, RepoName},
};
use anyhow::Result;
use async_trait::async_trait;
use beep_proto::{ProjectIssue, ProjectPullRequest};
use iroh::EndpointId;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use url::Url;

/// How long one host command may take.
pub const HOST_LIMIT: Duration = Duration::from_secs(30);

/// Why a host could not be read.
#[derive(Debug, Error)]
pub enum SourceError {
    /// For the user, saying why.
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

/// A host command that did not exit 0.
#[derive(Debug, Error)]
#[error("the host command exited with {}: {tail}", code.map_or("no code".to_owned(), |code| code.to_string()))]
pub struct HostExit {
    /// `None` when it was killed or timed out.
    pub code: Option<i32>,
    /// The end of what it printed.
    pub tail: String,
}

/// What the issue modules read about one project, kept by the server in Postgres and by the
/// desktop in its mesh state.
#[async_trait]
pub trait ProjectStore: Send + Sync {
    fn name(&self) -> &RepoName;

    /// Where issues are kept, as the home checkout's setup says.
    fn issues_role(&self) -> Option<&IssueHost>;

    /// The machine of the project's home checkout.
    async fn home_machine(&self) -> Result<Option<EndpointId>>;

    /// The places the project was imported on, matched to it by name.
    async fn imports(&self) -> Result<Vec<ImportedPlace>>;

    /// The numbers of the issues on `host` that tasks of the project track.
    async fn tracked_issues(&self, host: &IssueHost) -> Result<Vec<u64>>;
}

/// The user's machines, for reading a host through one of them.
#[async_trait]
pub trait Machines: Send + Sync {
    /// A shell on `machine`, while it is online and offers one.
    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>>;

    /// Every machine's name, for messages; empty when they are unknown.
    async fn names(&self) -> Vec<MachineName>;
}

/// A machine the project was imported on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPlace {
    /// The repository's web page, which names its host.
    pub url: String,
    /// `None` when the machine is not known by an endpoint id; the page still names the host.
    pub machine: Option<EndpointId>,
}

/// A machine's id and the name its user gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineName {
    pub id: EndpointId,
    pub name: String,
}

/// Where a project's open issues are read: its host, via a shell on a machine with the project.
pub struct IssueSource {
    pub host: IssueHost,
    pub machine: EndpointId,
    pub shell: Arc<dyn Shell>,
}

/// The issues host a repository web page is on.
pub fn host_of_url(url: &str) -> Option<IssueHost> {
    IssueHost::of_hostname(Url::parse(url).ok()?.host_str()?)
}

/// A project's issues host and the machines that can reach it.
pub struct HostAndMachines {
    pub host: IssueHost,
    /// The home checkout's machine, else the machines it was imported on.
    pub machines: Vec<EndpointId>,
    /// Whether the machines are the home checkout's.
    pub home: bool,
}

/// From the home checkout's issues role and machine, else the imported web page's host and
/// machines. `None` without a GitHub or Forgejo host.
pub async fn host_and_machines(store: &dyn ProjectStore) -> Result<Option<HostAndMachines>> {
    if let Some(machine) = store.home_machine().await? {
        return Ok(store.issues_role().cloned().map(|host| HostAndMachines {
            host,
            machines: vec![machine],
            home: true,
        }));
    }

    let imports = store.imports().await?;
    Ok(imports
        .iter()
        .find_map(|import| host_of_url(&import.url))
        .map(|host| HostAndMachines {
            host,
            machines: imports.iter().filter_map(|import| import.machine).collect(),
            home: false,
        }))
}

/// The project's issues host, if it has one.
pub async fn issue_host(store: &dyn ProjectStore) -> Result<Option<IssueHost>> {
    Ok(host_and_machines(store).await?.map(|found| found.host))
}

/// The first online place to read the project's issues (see [`issue_sources`]).
pub async fn issue_source(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Option<IssueSource>, SourceError> {
    Ok(issue_sources(store, machines)
        .await?
        .and_then(|sources| sources.into_iter().next()))
}

/// Every online place to read the project's issues: the home machine, else import machines by
/// id. `None` without a GitHub or Forgejo host; refused when none is online.
pub async fn issue_sources(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Option<Vec<IssueSource>>, SourceError> {
    let Some(found) = host_and_machines(store).await? else {
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
            machines.shell(machine).map(|shell| IssueSource {
                host: found.host.clone(),
                machine: *machine,
                shell,
            })
        })
        .collect();
    if sources.is_empty() {
        return Err(SourceError::Refused(if found.home {
            "The home checkout's machine is offline or offers no shell.".to_owned()
        } else {
            "No machine the project is on is online.".to_owned()
        }));
    }

    Ok(Some(sources))
}

/// Every open issue on a project's host, as it was read.
#[derive(Debug, Clone)]
pub struct HostIssues {
    pub host: IssueHost,
    pub open: Vec<OpenIssue>,
}

/// Every open issue on the project's host, tracked or not; `None` without a host.
pub async fn read_open(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Option<HostIssues>, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Ok(None);
    };
    let (source, open) = read_any(
        machines,
        &sources,
        store.name(),
        HostCommand::ListOpenIssues,
        parse_open_issues,
    )
    .await?;
    Ok(Some(HostIssues {
        host: source.host.clone(),
        open,
    }))
}

/// The issues of `open` that no task tracks, newest first.
pub fn untracked(open: Vec<OpenIssue>, tracked: &[u64]) -> Vec<OpenIssue> {
    let mut left: Vec<OpenIssue> = open
        .into_iter()
        .filter(|issue| !tracked.contains(&issue.number))
        .collect();
    left.sort_by(|a, b| {
        b.opened_at
            .cmp(&a.opened_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    left
}

/// The issues of `read` that no task of the project tracks now, newest first.
pub async fn untracked_now(
    store: &dyn ProjectStore,
    read: &HostIssues,
) -> Result<Vec<OpenIssue>, SourceError> {
    let tracked = store.tracked_issues(&read.host).await?;
    Ok(untracked(read.open.clone(), &tracked))
}

/// Open issues on the project's host no task tracks, newest first; empty without a host.
pub async fn list_open(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Vec<OpenIssue>, SourceError> {
    match read_open(store, machines).await? {
        Some(read) => untracked_now(store, &read).await,
        None => Ok(Vec::new()),
    }
}

/// An open pull request on a project's host, and whether it is the user's.
#[derive(Debug, Clone)]
pub struct ListedPr {
    pub pr: OpenPr,
    /// Opened by the CLI's signed-in account; false when that is unknown.
    pub mine: bool,
}

/// Open pull requests on the project's host, any author, newest first; empty without a host.
pub async fn list_open_prs(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Vec<ListedPr>, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Ok(Vec::new());
    };
    let read = read_sources(
        &sources,
        store.name(),
        HostCommand::ListOpenPrs,
        parse_open_prs,
    )
    .await;
    let (source, mut prs) = match read {
        Ok(answered) => answered,
        Err(failures) => {
            let set_up = set_up_on(&sources, store.name(), &failures).await;
            // teacli answers a repository without pull requests with "not found", which on a
            // machine that has the host set up means none.
            let none =
                sources
                    .iter()
                    .zip(&failures)
                    .zip(&set_up)
                    .any(|((source, error), set_up)| {
                        *set_up
                            && matches!(source.host, IssueHost::Forgejo(_))
                            && is_not_found(error)
                    });
            if none {
                return Ok(Vec::new());
            }

            return Err(failed_everywhere(machines, &sources, &failures, &set_up).await);
        }
    };
    // Asked on the machine that listed them, whose CLI is known to work.
    let whoami = run_host(
        source.shell.as_ref(),
        &source.host,
        store.name(),
        HostCommand::WhoAmI,
    )
    .await;
    let login = match whoami.and_then(|out| parse_login(&out)) {
        Ok(login) => Some(login),
        Err(error) => {
            log::info!(
                "projects: the host's account is unknown, so no PR counts as mine: {error:#}"
            );
            None
        }
    };
    prs.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    Ok(prs
        .into_iter()
        .map(|pr| ListedPr {
            mine: login.as_deref().is_some_and(|login| {
                !pr.author.is_empty() && login.eq_ignore_ascii_case(&pr.author)
            }),
            pr,
        })
        .collect())
}

/// What the machine holding a project says about the user and the project's repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineView {
    /// The host's name as the items' web addresses spell it.
    pub host: String,
    /// The account the machine's CLI is signed in as.
    pub login: String,
    /// Whether that account may push; `None` when the machine could not say.
    pub can_push: Option<bool>,
}

/// Asks the machine that answers for the project, through the shell its lists come from. `None`
/// without a host or an online machine that knows its account.
pub async fn machine_view(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Option<MachineView> {
    let sources = issue_sources(store, machines).await.ok()??;
    let (source, login) = read_sources(&sources, store.name(), HostCommand::WhoAmI, parse_login)
        .await
        .ok()?;
    let can_push = run_host(
        source.shell.as_ref(),
        &source.host,
        store.name(),
        HostCommand::CanPush,
    )
    .await
    .and_then(|out| parse_can_push(&out))
    .map_err(|error| log::info!("projects: the machine could not say if it can push: {error:#}"))
    .ok();
    let host = match &source.host {
        IssueHost::Github => "github.com".to_owned(),
        IssueHost::Forgejo(host) => host.as_ref().to_owned(),
    };
    Some(MachineView {
        host,
        login,
        can_push,
    })
}

/// An open issue as the apps list it.
pub fn issue_view(issue: OpenIssue) -> ProjectIssue {
    ProjectIssue {
        number: issue.number,
        title: issue.title,
        labels: issue.labels,
        author: issue.author,
        opened_at: issue.opened_at.into(),
        comments: issue.comments,
        url: issue.url,
    }
}

/// An open pull request as the apps list it.
pub fn pull_request_view(listed: ListedPr) -> ProjectPullRequest {
    let pr = listed.pr;
    ProjectPullRequest {
        number: pr.number,
        title: pr.title,
        url: pr.url,
        author: pr.author,
        draft: pr.draft,
        mine: listed.mine,
        created_at: pr.created_at.into(),
        updated_at: pr.updated_at.into(),
    }
}

/// The branch pull request `number` is made from, read from the host now.
pub async fn pull_request_head(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
    number: u64,
) -> Result<PrHead, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Err(SourceError::Refused(
            "The project has no GitHub or Forgejo host to read pull requests from.".into(),
        ));
    };
    let (_, head) = read_any(
        machines,
        &sources,
        store.name(),
        HostCommand::ViewPrHead { number },
        parse_pr_head,
    )
    .await?;
    Ok(head)
}

#[cfg(test)]
#[path = "tests/issues.rs"]
mod tests;
