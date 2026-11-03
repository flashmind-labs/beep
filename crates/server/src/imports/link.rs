//! Links an existing checkout of the repo or fork instead of cloning: adds missing remotes and PR branches.

use super::{
    clone::{self, Link, NewRemote},
    fork::UsedFork,
    preview::{FolderRemotes, parse_remotes, remotes_script, repo_remote},
    repo::{Host, RepoRef},
};
use crate::{
    nodes::Online,
    projects::branches::{READ_LIMIT, run_script},
    store::NodeRow,
};
use secrecy::{ExposeSecret, SecretString};
use std::time::Duration;

/// The longest fetching the fork and the repository may take.
const LINK_LIMIT: Duration = Duration::from_secs(30 * 60);
/// The most output kept while linking; the end explains a failure.
const MAX_OUTPUT: usize = 64 * 1024;

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
pub(super) async fn link_fork(
    node: &Online,
    row: &NodeRow,
    repo: &RepoRef,
    fork: &UsedFork,
    folder: &str,
    token: Option<&SecretString>,
) {
    let read = match run_script(node, &remotes_script(&[folder.to_owned()]), READ_LIMIT).await {
        Ok(out) => parse_remotes(&out),
        Err(e) => {
            log::info!("link: remotes not read on {}: {e:#}", row.name);
            return;
        }
    };
    let Some(remotes) = read.first() else {
        return;
    };
    let Some(plan) = plan(remotes, repo, &fork.repo) else {
        log::info!(
            "link: {folder} on {} has an upstream or fork remote of its own",
            row.name
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
    let mut output = String::new();
    let mut on_output = |text: &str| {
        output.push_str(text);
        if output.len() > MAX_OUTPUT {
            output = clone::tail(&output);
        }
    };
    match node
        .exec(script.expose_secret(), LINK_LIMIT, &mut on_output)
        .await
    {
        Ok(exited) if clone::linked(&output, exited.exit_code) => {}
        Ok(_) => log::info!(
            "link: {folder} on {} not linked to its fork: {}",
            row.name,
            clone::tail(&output)
        ),
        Err(e) => log::info!("link: could not reach {}: {e:#}", row.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::model::RepoName;

    fn folder(remotes: &[(&str, &str)]) -> FolderRemotes {
        FolderRemotes {
            folder: "/srv/widget".into(),
            remotes: remotes
                .iter()
                .map(|(name, url)| ((*name).to_owned(), (*url).to_owned()))
                .collect(),
        }
    }

    fn repos() -> (RepoRef, RepoRef) {
        let repo = RepoRef::parse("acme/widget", &[]).expect("a repo");
        let fork = repo
            .sibling(&RepoName::parse("octo/widget").expect("a name"))
            .expect("a fork");
        (repo, fork)
    }

    #[test]
    fn a_checkout_of_the_fork_gets_upstream_unless_the_name_is_taken() {
        let (repo, fork) = repos();
        let plan = plan(
            &folder(&[("origin", "https://github.com/octo/widget")]),
            &repo,
            &fork,
        )
        .expect("a plan");
        assert_eq!(plan.fork_remote, "origin");
        assert_eq!(plan.upstream_remote, "upstream");
        assert_eq!(plan.add[0].url, "https://github.com/acme/widget.git");
        let both = plan_of(&[
            ("origin", "https://github.com/octo/widget"),
            ("up", "https://github.com/acme/widget"),
        ]);
        assert!(both.add.is_empty());
        assert_eq!(both.upstream_remote, "up");
        let (repo, fork) = repos();
        assert_eq!(
            super::plan(
                &folder(&[
                    ("origin", "https://github.com/octo/widget"),
                    ("upstream", "https://github.com/someone/widget"),
                ]),
                &repo,
                &fork
            ),
            None
        );
    }

    fn plan_of(remotes: &[(&str, &str)]) -> Plan {
        let (repo, fork) = repos();
        plan(&folder(remotes), &repo, &fork).expect("a plan")
    }
}
