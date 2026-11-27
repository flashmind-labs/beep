//! Starts an agent on the bridge its kind names: the node's own Claude Code
//! and Codex bridges, or a relay into WSL, or the agent's own process.

use super::{Bridge, claude_direct, codex_direct, launch::{Cli, Running, STDERR_LINES, Started, pipe, spawn}};
use crate::machine::wsl;
use anyhow::Result;
use beep_proto::team::TeamRepo;
use std::future::Future;
use url::Url;
use tokio::{
    io::DuplexStream,
    process::Command,
    sync::{mpsc, oneshot},
};
use tokio_util::{either::Either, task::AbortOnDropHandle};

/// Starts the agent `process` sets up, on `bridge` when given: the bridge
/// starts the CLI `process` names, with its arguments after the bridge's,
/// its environment and its folder.
pub fn start(process: Command, bridge: Option<&Bridge>) -> Result<Started> {
    let (lines, stderr) = mpsc::channel(STDERR_LINES);

    match bridge {
        None => {
            let (process, stdin, stdout) = spawn(process, "the agent", lines)?;
            Ok(Started {
                stdin: Either::Left(stdin),
                stdout: Either::Left(stdout),
                stderr,
                running: Running {
                    process: Some(process),
                    task: None,
                    started: None,
                },
            })
        }
        Some(Bridge::Relay { wsl }) => {
            let cli = Cli::new(process, lines.clone(), None);
            let cwd = cli.cwd()?;
            let relayed = cli.command(Some(wsl), &cwd, &cli.args())?;
            let (process, agent_in, agent_out) = spawn(relayed, "the agent in WSL", lines)?;
            let (stdin, client_in) = pipe();
            let (client_out, stdout) = pipe();
            let relay = async move {
                if let Err(e) = wsl::relay(client_in, client_out, agent_in, agent_out).await {
                    log::warn!("the relay to WSL failed: {e:#}");
                }
            };
            Ok(Started {
                stdin: Either::Right(stdin),
                stdout: Either::Right(stdout),
                stderr,
                running: Running {
                    process: Some(process),
                    task: Some(AbortOnDropHandle::new(tokio::spawn(relay))),
                    started: None,
                },
            })
        }
        Some(Bridge::Claude { wsl, home }) => {
            let (wsl, home) = (wsl.clone(), home.clone());
            bridged(process, lines, stderr, |input, output, cli| {
                claude_direct::serve(input, output, cli, wsl, home)
            })
        }
        Some(Bridge::Codex { wsl }) => {
            let wsl = wsl.clone();
            bridged(process, lines, stderr, |input, output, cli| {
                codex_direct::serve(input, output, cli, wsl)
            })
        }
    }
}

/// The agent on the bridge `serve` runs as a task.
fn bridged<F, S>(
    process: Command,
    lines: mpsc::Sender<String>,
    stderr: mpsc::Receiver<String>,
    serve: S,
) -> Result<Started>
where
    S: FnOnce(DuplexStream, DuplexStream, Cli) -> F,
    F: Future<Output = Result<()>> + Send + 'static,
{
    let (stdin, input) = pipe();
    let (output, stdout) = pipe();
    let (pid, started) = oneshot::channel();
    let served = serve(input, output, Cli::new(process, lines.clone(), Some(pid)));
    let task = async move {
        if let Err(e) = served.await {
            log::warn!("the agent's bridge failed: {e:#}");
            // The session reads it once the bridge ends; a full channel
            // already says why.
            let _ = lines.try_send(format!("{e:#}"));
        }
    };

    Ok(Started {
        stdin: Either::Right(stdin),
        stdout: Either::Right(stdout),
        stderr,
        running: Running {
            process: None,
            task: Some(AbortOnDropHandle::new(tokio::spawn(task))),
            started: Some(started),
        },
    })
}

/// Whether `url` is the web page of issue `number` of `repo`, as the forge
/// lists it: on its host, ending in `/<org>/<name>/issues/<number>`.
pub(crate) fn issue_of_repo(repo: &TeamRepo, number: u64, url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let wanted = format!("/{}/issues/{number}", repo.repo).to_lowercase();

    matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some_and(|host| host.eq_ignore_ascii_case(repo.hostname()))
        && url.path().to_lowercase().ends_with(&wanted)
}

/// The issue of `repo` that `prompt` names: the first URL in it that
/// `issue_of_repo` accepts, else the first `issue #<n>` (any case). A bare
/// `#<n>` names nothing: it may be a pull request or another repository's.
pub(crate) fn issue_of_prompt(repo: &TeamRepo, prompt: &str) -> Option<u64> {
    let urls = prompt
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '[' | ']' | '"' | '\''))
        .map(|word| word.trim_end_matches(['.', ',', ';', ':', '!', '?']))
        .filter(|word| word.contains("/issues/"))
        .find_map(|url| {
            let (_, tail) = url.rsplit_once("/issues/")?;
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            let number = digits.parse().ok()?;

            issue_of_repo(repo, number, url).then_some(number)
        });

    urls.or_else(|| issue_number_in_words(prompt))
}

/// The first `issue #<n>` in `text`, in any case, not the end of a longer
/// word.
fn issue_number_in_words(text: &str) -> Option<u64> {
    const WORDS: &str = "issue #";
    let lower = text.to_ascii_lowercase();

    lower.match_indices(WORDS).find_map(|(at, _)| {
        let before = lower[..at].chars().next_back();

        if before.is_some_and(char::is_alphanumeric) {
            return None;
        }

        let digits: String = lower[at + WORDS.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();

        digits.parse().ok()
    })
}
