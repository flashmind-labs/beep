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

    // Read in process: no git process for each folder the events watch.
    let root = root.to_owned();
    let found = tokio::task::spawn_blocking(move || {
        let repo = crate::agents::found::repository(&root)?;
        Some((repo.git_dir().to_owned(), repo.common_dir().to_owned()))
    })
    .await?;
    let Some((git, common)) = found else {
        return Ok(Vec::new());
    };

    let git = tokio::fs::canonicalize(git).await?;
    let common = tokio::fs::canonicalize(common).await?;
    // Everything the Git projections and diffs read besides the working tree:
    // the branch, index, refs, upstream settings, worktrees and exclusions.
    Ok(vec![
        git.join("HEAD"),
        git.join("index"),
        git.join("commondir"),
        common.join("refs"),
        common.join("packed-refs"),
        common.join("config"),
        common.join("worktrees"),
        common.join("info").join("exclude"),
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

    if let Some(output) = read_in_process(&root, &query).await {
        return Ok(output);
    }

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

/// The state and branch reads, which subscriptions repeat on every ref
/// change, answered by gitoxide with the text their scripts print. `None`
/// for the other reads, or when gitoxide cannot say: the script answers.
async fn read_in_process(root: &Path, query: &GitRead) -> Option<GitOutput> {
    let (root, query) = (root.to_owned(), query.clone());
    let output = tokio::task::spawn_blocking(move || match query {
        GitRead::State => Some(super::git_reads::state(&root)),
        GitRead::Branches { fallback } => Some(super::git_reads::branches(&root, &fallback)),
        GitRead::History { .. } | GitRead::Files { .. } => None,
    })
    .await
    .ok()??;

    match output {
        Ok(output) => Some(GitOutput {
            exit_code: Some(0),
            timed_out: false,
            output,
        }),
        Err(e) => {
            log::debug!("Git read falls back to git: {e:#}");
            None
        }
    }
}

#[cfg(all(test, unix))]
#[path = "../../tests/link_events_git.rs"]
mod tests;
