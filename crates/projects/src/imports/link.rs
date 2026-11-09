//! Links an existing checkout of the repo or fork instead of cloning: adds missing remotes and PR branches.

use super::{
    clone::{self, Link, NewRemote},
    fork::UsedFork,
    machine::Machine,
    preview::{FolderRemotes, parse_remotes, remotes_script, repo_remote},
    repo::{Host, RepoRef},
};
use crate::branches::{READ_LIMIT, run_script};
use secrecy::{ExposeSecret, SecretString};
use std::time::Duration;

/// The longest fetching the fork and the repository may take.
const LINK_LIMIT: Duration = Duration::from_secs(30 * 60);

/// The checkout's remote names for the repo and fork, and the ones to add.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    upstream_remote: String,
    fork_remote: String,
    add: Vec<NewRemote>,
}

/// The remotes `folder` needs for `repo` and `fork`; `None` when a name to add is taken.
fn plan(folder: &FolderRemotes, repo: &RepoRef, fork: &RepoRef) -> Option<Plan> {
    let mut add = Vec::new();
    let mut remote_for = |target: &RepoRef, name: &str| match repo_remote(folder, target) {
        Some(found) => Some(found),
        None if folder.remotes.iter().any(|(taken, _)| taken == name) => None,
        None => {
            add.push(NewRemote {
                name: name.to_owned(),
                url: target.clone_url(),
            });
            Some(name.to_owned())
        }
    };
    let upstream_remote = remote_for(repo, "upstream")?;
    let fork_remote = remote_for(fork, "fork")?;
    Some(Plan {
        upstream_remote,
        fork_remote,
        add,
    })
}

/// Links the checkout at `folder` to `fork`; a failure is logged and the import goes on.
pub async fn link_fork(
    machine: &Machine,
    repo: &RepoRef,
    fork: &UsedFork,
    folder: &str,
    token: Option<&SecretString>,
) {
    let Ok(shell) = machine.shell() else {
        return;
    };
    let read = match run_script(shell, &remotes_script(&[folder.to_owned()]), READ_LIMIT).await {
        Ok(out) => parse_remotes(&out),
        Err(e) => {
            log::info!("link: remotes not read on {}: {e:#}", machine.name);
            return;
        }
    };
    let Some(remotes) = read.first() else {
        return;
    };
    let Some(plan) = plan(remotes, repo, &fork.repo) else {
        log::info!(
            "link: {folder} on {} has an upstream or fork remote of its own",
            machine.name
        );
        return;
    };
    let script = clone::link_script(&Link {
        folder,
        token,
        github: repo.host == Host::GitHub,
        add: &plan.add,
        fork_remote: &plan.fork_remote,
        upstream_remote: &plan.upstream_remote,
        branches: &fork.found.branches,
    });

    match shell.run(script.expose_secret(), LINK_LIMIT).await {
        Ok(ran) if clone::linked(&ran.output, ran.exit_code) => {}
        Ok(ran) => log::info!(
            "link: {folder} on {} not linked to its fork: {}",
            machine.name,
            clone::tail(&ran.output)
        ),
        Err(e) => log::info!("link: could not reach {}: {e:#}", machine.name),
    }
}

#[cfg(test)]
#[path = "../tests/imports_link.rs"]
mod tests;
