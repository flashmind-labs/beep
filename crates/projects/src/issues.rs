//! Reading a project's issues host through its machines: which machines can reach it, and trying
//! each in turn.

mod command;
mod gate;
mod host;

pub use command::{HOST_LIMIT, HostExit, HostTimeout, IssueSource, SourceError, run_rendered};
pub use gate::{HostGates, Unreachable};
pub(crate) use host::read_first;
pub use host::{MachineName, Machines, clipped, head, host_failed, read_any, run_source};
use host::{failed_everywhere, is_not_found, read_sources, set_up_on};

use crate::{
    commands::{
        HostCommand, OpenIssue, OpenPr, parse_can_push, parse_login, parse_open_issues,
        parse_open_prs, parse_pr_head,
    },
    model::{IssueHost, RepoName},
};
use anyhow::Result;
use async_trait::async_trait;
use beep_proto::{ProjectIssue, ProjectPullRequest, PullRequestHead};
use iroh::EndpointId;
use std::collections::HashSet;
use url::Url;

/// What the issue modules read about one project, kept by the desktop in its mesh state.
#[async_trait]
pub trait ProjectStore: Send + Sync {
    fn name(&self) -> &RepoName;

    /// The places the project was imported on, matched to it by name.
    async fn imports(&self) -> Result<Vec<ImportedPlace>>;
}

/// A machine the project was imported on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPlace {
    /// The repository's web page, which names its host.
    pub url: String,
    /// `None` when the machine is not known by an endpoint id; the page still names the host.
    pub machine: Option<EndpointId>,
}

/// The issues host a repository web page is on.
pub fn host_of_url(url: &str) -> Option<IssueHost> {
    IssueHost::of_hostname(Url::parse(url).ok()?.host_str()?)
}

/// A project's issues host and the machines that can reach it.
struct HostAndMachines {
    pub host: IssueHost,
    /// The machines it was imported on.
    pub machines: Vec<EndpointId>,
}

/// From the imported web page's host and machines. `None` without a GitHub or Forgejo host.
async fn host_and_machines(store: &dyn ProjectStore) -> Result<Option<HostAndMachines>> {
    let imports = store.imports().await?;
    Ok(imports
        .iter()
        .find_map(|import| host_of_url(&import.url))
        .map(|host| HostAndMachines {
            host,
            machines: imports.iter().filter_map(|import| import.machine).collect(),
        }))
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

/// Every online place to read the project's issues: the machines it was imported on, by id. The machine that last answered for the host comes first and machines that cannot reach it
/// now come last. `None` without a GitHub or Forgejo host; refused when none is online.
pub async fn issue_sources(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Option<Vec<IssueSource>>, SourceError> {
    let Some(found) = host_and_machines(store).await? else {
        return Ok(None);
    };
    let mut seen = Vec::new();
    let mut sources: Vec<IssueSource> = found
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
        return Err(SourceError::Refused(
            "No machine the project is on is online.".to_owned(),
        ));
    }

    if let Some(gates) = machines.gates() {
        let preferred = gates.preferred(&found.host);
        sources.sort_by_key(|source| {
            (
                Some(source.machine) != preferred,
                gates.is_paused(source.machine, &source.host),
            )
        });
    }

    Ok(Some(sources))
}

/// Open issues on the project's host, newest first; empty without a host.
pub async fn list_open(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<Vec<OpenIssue>, SourceError> {
    let Some(sources) = issue_sources(store, machines).await? else {
        return Ok(Vec::new());
    };
    let (source, mut open) = read_any(
        machines,
        &sources,
        store.name(),
        HostCommand::ListOpenIssues,
        parse_open_issues,
    )
    .await?;

    if matches!(source.host, IssueHost::Github) {
        // Asked on the machine that listed them. Without it the issues still list, untagged.
        let linked = run_source(
            machines,
            source,
            store.name(),
            HostCommand::ListLinkedIssues,
        )
        .await
        .and_then(|out| Ok(serde_json::from_str::<HashSet<u64>>(out.trim())?));
        match linked {
            Ok(linked) => {
                for issue in &mut open {
                    issue.linked_pr = linked.contains(&issue.number);
                }
            }
            Err(error) => log::info!("projects: issues linked to a PR are unknown: {error:#}"),
        }
    }

    open.sort_by(|a, b| {
        b.opened_at
            .cmp(&a.opened_at)
            .then_with(|| b.number.cmp(&a.number))
    });
    Ok(open)
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
        machines,
        &sources,
        store.name(),
        HostCommand::ListOpenPrs,
        parse_open_prs,
    )
    .await;
    let (source, mut prs) = match read {
        Ok(answered) => answered,
        Err(failures) => {
            let set_up = set_up_on(machines, &sources, store.name(), &failures).await;
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
    let whoami = run_source(machines, source, store.name(), HostCommand::WhoAmI).await;
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
    let (source, login) = read_sources(
        machines,
        &sources,
        store.name(),
        HostCommand::WhoAmI,
        parse_login,
    )
    .await
    .ok()?;
    let can_push = run_source(machines, source, store.name(), HostCommand::CanPush)
        .await
        .and_then(|out| parse_can_push(&out))
        .map_err(|error| {
            log::info!("projects: the machine could not say if it can push: {error:#}")
        })
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
        linked_pr: issue.linked_pr,
        assignees: issue.assignees,
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
) -> Result<PullRequestHead, SourceError> {
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
