//! Running issue-host commands and reporting failures across machines.

use super::{HOST_LIMIT, HostExit, IssueHost, IssueSource, Machines, RepoName, Shell, SourceError};
use crate::commands::HostCommand;
use anyhow::Result;

/// teacli's exit code for a missing resource.
const TEACLI_NOT_FOUND: i32 = 3;

/// Runs `command` against `repo`'s `host`, failing unless it exits 0.
pub async fn run_host(
    shell: &dyn Shell,
    host: &IssueHost,
    repo: &RepoName,
    command: HostCommand<'_>,
) -> Result<String> {
    run_rendered(shell, &command.render(host, repo)).await
}

/// Runs the host command `rendered`, failing unless it exits 0.
pub async fn run_rendered(shell: &dyn Shell, rendered: &str) -> Result<String> {
    let out = shell.run(rendered, HOST_LIMIT).await?;

    if out.timed_out {
        anyhow::bail!(
            "the host command timed out after {} seconds",
            HOST_LIMIT.as_secs()
        );
    }

    match out.exit_code {
        Some(0) => Ok(out.output),
        code => {
            let tail: String = out
                .output
                .chars()
                .rev()
                .take(400)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Err(HostExit {
                code,
                tail: tail.trim().to_owned(),
            }
            .into())
        }
    }
}

/// Runs a host command on each of `items`' shells in turn, so one broken CLI does not hide the
/// answer. `command` renders the script for an item and `parse` reads what it printed. Gives the
/// item that answered; when none does, every error, in order.
pub async fn read_first<S, T>(
    items: &[S],
    shell_of: impl Fn(&S) -> &dyn Shell,
    command: impl Fn(&S) -> String,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&S, T), Vec<anyhow::Error>> {
    let mut failures = Vec::new();

    for item in items {
        let read = run_rendered(shell_of(item), &command(item))
            .await
            .and_then(|out| parse(&out));
        match read {
            Ok(value) => return Ok((item, value)),
            Err(error) => {
                log::info!("projects: reading failed on one machine: {error:#}");
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
    match read_sources(sources, repo, command, parse).await {
        Ok(answered) => Ok(answered),
        Err(failures) => {
            let set_up = set_up_on(sources, repo, &failures).await;
            Err(failed_everywhere(machines, sources, &failures, &set_up).await)
        }
    }
}

pub(super) async fn read_sources<'s, T>(
    sources: &'s [IssueSource],
    repo: &RepoName,
    command: HostCommand<'_>,
    parse: impl Fn(&str) -> Result<T>,
) -> Result<(&'s IssueSource, T), Vec<anyhow::Error>> {
    read_first(
        sources,
        |source| source.shell.as_ref(),
        |source| command.render(&source.host, repo),
        parse,
    )
    .await
}

/// For each of the sources that failed, in order, whether its machine has the host set up: it
/// answers [`HostCommand::WhoAmI`].
pub(super) async fn set_up_on(
    sources: &[IssueSource],
    repo: &RepoName,
    failures: &[anyhow::Error],
) -> Vec<bool> {
    let mut set_up = Vec::new();

    for source in sources.iter().take(failures.len()) {
        let whoami = run_host(
            source.shell.as_ref(),
            &source.host,
            repo,
            HostCommand::WhoAmI,
        )
        .await;
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
    let mut end = text.len().min(max);

    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
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
