use crate::agents::{Agents, reach::Access};
use anyhow::{Context, Result, bail, ensure};
use beep_projects::{
    branches, checkout_sync, for_machine,
    history::{self, CommitId, HistoryAsk},
};
use beep_proto::node::{
    events::{GitOutput, GitRead},
    local_link, shell,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::task::AbortOnDropHandle;

/// Worktree metadata can live outside the checkout. Avoid watching the object database.
pub(super) async fn watch_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut repository = false;

    for folder in root.ancestors() {
        if tokio::fs::symlink_metadata(folder.join(".git"))
            .await
            .is_ok()
        {
            repository = true;
            break;
        }
    }

    if !repository {
        return Ok(Vec::new());
    }

    let output = tokio::time::timeout(
        Duration::from_secs(5),
        crate::run::process::command("git")
            .arg("-C")
            .arg(root)
            .args([
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("finding Git metadata timed out")?;
    let output = match output {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        other => other?,
    };

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let text = String::from_utf8(output.stdout).context("Git metadata path is not UTF-8")?;
    let mut paths = text.lines();
    let git = paths
        .next()
        .context("Git did not name its metadata folder")?;
    let common = paths.next().context("Git did not name its common folder")?;
    let git = tokio::fs::canonicalize(root.join(git)).await?;
    let common = tokio::fs::canonicalize(root.join(common)).await?;
    Ok(vec![
        git.join("HEAD"),
        git.join("index"),
        git.join("commondir"),
        common.join("refs"),
        common.join("packed-refs"),
    ])
}

fn script(root: &str, query: GitRead) -> Result<String> {
    Ok(match query {
        GitRead::State => checkout_sync::local_state_script(root),
        GitRead::Branches { fallback } => branches::branches_script(root, &fallback),
        GitRead::History {
            revision,
            offset,
            search,
        } => {
            let revision = revision
                .as_deref()
                .map(str::parse::<CommitId>)
                .transpose()?;
            history::history_script(&HistoryAsk {
                folder: root,
                revision: revision.as_ref(),
                offset,
                search: &search,
            })
        }
        GitRead::Files { commit } => history::files_script(root, &commit.parse::<CommitId>()?),
    })
}

pub(super) async fn read(agents: &Agents, root: &str, query: GitRead) -> Result<GitOutput> {
    let root = agents.reachable(root, Access::Read).await?;
    let root = root.to_str().context("the Git folder is not UTF-8")?;
    let command = for_machine(&script(root, query)?, cfg!(windows)).into_owned();
    let link = local_link();
    let _task = AbortOnDropHandle::new(tokio::spawn(crate::run::shell::serve(
        link.node.0,
        link.node.1,
    )));
    let (mut tx, mut rx) = link.caller;
    tx.send(&shell::Request::Run {
        command,
        timeout_secs: 40,
    })
    .await?;
    let mut output = String::new();

    loop {
        match rx
            .recv::<shell::Event>()
            .await?
            .context("Git read closed before its result")?
        {
            shell::Event::Output { text } => output.push_str(&text),
            shell::Event::Done {
                exit_code,
                timed_out,
                truncated,
                ..
            } => {
                ensure!(!truncated, "Git projection output exceeded its limit");
                return Ok(GitOutput {
                    exit_code,
                    timed_out,
                    output,
                });
            }
            _ => bail!("unexpected Git shell result"),
        }
    }
}

#[cfg(all(test, unix))]
#[path = "../../tests/link_events_git.rs"]
mod tests;
