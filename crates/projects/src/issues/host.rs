//! Running issue-host commands and reporting failures across machines.

use super::{
    command::{HostExit, IssueSource, SourceError, run_rendered},
    gate::{HostGates, Unreachable, is_unreachable},
};
use crate::{
    Shell,
    commands::HostCommand,
    model::{IssueHost, RepoName},
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use iroh::EndpointId;
use std::{future::Future, sync::Arc};

/// The user's machines, for reading a host through one of them.
#[async_trait]
pub trait Machines: Send + Sync {
    /// A shell on `machine`, while it is online and offers one.
    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>>;

    /// Every machine's name, for messages; empty when they are unknown.
    async fn names(&self) -> Vec<MachineName>;

    /// The hosts each machine could not reach lately, kept across reads; `None` asks every time.
    fn gates(&self) -> Option<&HostGates> {
        None
    }
}

/// A machine's id and the name its user gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineName {
    pub id: EndpointId,
    pub name: String,
}

/// teacli's exit code for a missing resource.
const TEACLI_NOT_FOUND: i32 = 3;

/// Runs `command` against `source`'s host through `machines`' gates, so a host its machine could
/// not reach is left alone for a while and identical reads at once run the CLI once.
pub async fn run_source(
    machines: &dyn Machines,
    source: &IssueSource,
    repo: &RepoName,
    command: HostCommand<'_>,
) -> Result<String> {
    let rendered = command
        .render(&source.host, repo)
        .ok_or_else(|| anyhow!("this host cannot answer {command:?}"))?;

    match machines.gates() {
        Some(gates) => gates.run(source, &rendered, command.only_reads()).await,
        None => run_rendered(source.shell.as_ref(), &rendered).await,
    }
}

/// Runs a host command for each of `items` in turn, so one broken CLI does not hide the answer.
/// `run` runs the item's command and `parse` reads what it printed. Gives the item that
/// answered; when none does, every error, in order. A host a gate found unreachable was logged by
/// the gate, so it is not logged again here.
pub async fn read_first<'s, S, T, F>(
    items: &'s [S],
    run: impl Fn(&'s S) -> F,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&'s S, T), Vec<anyhow::Error>>
where
    F: Future<Output = Result<String>>,
{
    let mut failures = Vec::new();

    for item in items {
        let read = run(item).await.and_then(|out| parse(&out));
        match read {
            Ok(value) => return Ok((item, value)),
            Err(error) => {
                if error.downcast_ref::<Unreachable>().is_none() {
                    log::info!("projects: reading failed on one machine: {error:#}");
                }

                failures.push(error);
            }
        }
    }

    Err(failures)
}

/// Tries `command` on each source in turn so one broken CLI does not hide the list. Gives the
/// source that answered; when all fail, the errors of the machines that have the host set up.
pub async fn read_any<'s, T>(
    machines: &dyn Machines,
    sources: &'s [IssueSource],
    repo: &RepoName,
    command: HostCommand<'_>,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&'s IssueSource, T), SourceError> {
    match read_sources(machines, sources, repo, command, parse).await {
        Ok(answered) => Ok(answered),
        Err(failures) => {
            let set_up = set_up_on(machines, sources, repo, &failures).await;
            Err(failed_everywhere(machines, sources, &failures, &set_up).await)
        }
    }
}

pub(super) async fn read_sources<'s, T>(
    machines: &dyn Machines,
    sources: &'s [IssueSource],
    repo: &RepoName,
    command: HostCommand<'_>,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&'s IssueSource, T), Vec<anyhow::Error>> {
    read_first(
        sources,
        |source| run_source(machines, source, repo, command),
        parse,
    )
    .await
}

/// For each of the sources that failed, in order, whether its machine has the host set up: it
/// answers [`HostCommand::WhoAmI`]. A machine that could not reach the host is not asked, as the
/// check would fail the same way, and counts as without it.
pub(super) async fn set_up_on(
    machines: &dyn Machines,
    sources: &[IssueSource],
    repo: &RepoName,
    failures: &[anyhow::Error],
) -> Vec<bool> {
    let mut set_up = Vec::new();

    for (source, failure) in sources.iter().zip(failures) {
        if is_unreachable(failure) {
            set_up.push(false);
            continue;
        }

        let whoami = run_source(machines, source, repo, HostCommand::WhoAmI).await;
        set_up.push(whoami.is_ok());
    }

    set_up
}

/// The message for a read that failed on every source, naming only the machines in `set_up`.
pub(super) async fn failed_everywhere(
    machines: &dyn Machines,
    sources: &[IssueSource],
    failures: &[anyhow::Error],
    set_up: &[bool],
) -> SourceError {
    if let ([error], [_]) = (failures, sources) {
        return host_failed(error);
    }

    let Some(first) = sources.first() else {
        return SourceError::Refused("No machine to read the issues host on.".to_owned());
    };
    // Names only make the message clearer: without them it still says why.
    let names = machines.names().await;
    let tried: Vec<String> = sources
        .iter()
        .zip(failures)
        .zip(set_up)
        .filter(|(_, set_up)| **set_up)
        .map(|((source, error), _)| {
            let name = names
                .iter()
                .find(|machine| machine.id == source.machine)
                .map_or("a machine", |machine| machine.name.as_str());
            format!("{name}: {error:#}")
        })
        .collect();
    if tried.is_empty() {
        return SourceError::Refused(format!(
            "No online machine has {} set up.",
            host_label(&first.host)
        ));
    }

    SourceError::Refused(format!(
        "Could not read the issues host. {}",
        tried.join("; ")
    ))
}

fn host_label(host: &IssueHost) -> &str {
    match host {
        IssueHost::Github => "GitHub",
        IssueHost::Forgejo(host) => host.as_ref(),
    }
}

/// Whether `error` is teacli saying the resource is not found.
pub(super) fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<HostExit>()
        .is_some_and(|exit| exit.code == Some(TEACLI_NOT_FOUND))
}

pub fn host_failed(error: &anyhow::Error) -> SourceError {
    SourceError::Refused(format!("Could not read the issues host: {error:#}"))
}

/// The start of `text`, at most `max` bytes, cut on a character boundary.
pub fn head(text: &str, max: usize) -> &str {
    &text[..text.floor_char_boundary(max)]
}

/// `text` trimmed and cut to at most `max` bytes, "..." marking a cut.
pub fn clipped(text: &str, max: usize) -> String {
    let text = text.trim();
    let cut = head(text, max);
    if cut.len() < text.len() {
        format!("{}...", cut.trim_end())
    } else {
        cut.to_owned()
    }
}
