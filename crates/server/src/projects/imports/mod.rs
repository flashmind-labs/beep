//! Importing a project: cloning a repository onto a user's machine through its node's shell, with
//! the user's token. Projects are named `org/repo`, as the projects store names them.

mod checks;
mod copy;
mod create;
mod fork;
mod found;
mod gone;
mod host_repos;
mod link;
mod preview;
mod remove;
mod service;
mod tool;

pub use checks::Checkouts;
pub(crate) use copy::copy_project;
pub(crate) use create::{create_project, project_forges};
pub(crate) use found::MachineFound;
pub use host_repos::HostRepoCache;
pub(crate) use host_repos::host_repos;
pub(crate) use preview::{checkout_for_worktree, parse_remotes, preview, remotes_script};
pub(crate) use remove::{machine_removed, remove_project};
pub(crate) use service::{
    Import, browse, find_fork, found_projects, import, imported, project_machines,
};
pub(crate) use tool::ImportProjectTool;

pub(crate) use beep_projects::imports::{ImportError, clone, folder, place, repo};

use crate::account::credentials::{Opened, Slot, github_for};
use repo::{Host, RepoRef};
use secrecy::ExposeSecret;

/// The user's non-empty token for `repo`'s host (on GitHub, the owner's account).
pub(super) fn credential_of<'a>(repo: &RepoRef, credentials: &'a [Opened]) -> Option<&'a Opened> {
    match &repo.host {
        Host::GitHub => github_for(credentials, repo.org()),
        Host::Forgejo(url) => credentials
            .iter()
            .find(|opened| matches!(&opened.slot, Slot::Forgejo(saved) if saved == url)),
    }
    .filter(|opened| !opened.secret.expose_secret().is_empty())
}
