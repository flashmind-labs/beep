//! Importing a project: cloning a repository onto a user's machine through its node's shell, with
//! the user's token. Projects are named `org/repo`, as the projects store names them.

mod checks;
mod clone;
mod copy;
mod create;
mod folder;
mod fork;
mod found;
mod gone;
mod host_repos;
mod link;
pub(crate) mod place;
mod preview;
mod remove;
pub(crate) mod repo;
mod service;
mod tool;

pub use checks::Checkouts;
pub(crate) use copy::copy_project;
pub(crate) use create::{create_project, project_forges};
pub(crate) use folder::FolderPath;
pub(crate) use found::MachineFound;
pub use host_repos::HostRepoCache;
pub(crate) use host_repos::host_repos;
pub(crate) use preview::{checkout_for_worktree, parse_remotes, preview, remotes_script};
pub(crate) use remove::{machine_removed, remove_project};
pub(crate) use service::{
    Import, browse, find_fork, found_projects, import, imported, project_machines,
};
pub(crate) use tool::ImportProjectTool;

use super::git_items::HostError;
use thiserror::Error;

/// Why an import did not happen, in words for the user.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum ImportError {
    #[error("Give the repository as org/repo or as its URL, such as https://github.com/org/repo.")]
    Malformed,
    #[error(
        "{0} is not GitHub or a Forgejo server in Settings > Credentials. Add a token for it there first."
    )]
    UnknownHost(String),
    #[error("That machine is not one of yours.")]
    NoSuchMachine,
    #[error("{0} is offline.")]
    Offline(String),
    #[error("{0} cannot run commands, so it cannot clone a project.")]
    NoShell(String),
    #[error("None of your machines is online. Start beep-node on one, or add a machine.")]
    NoneOnline,
    #[error("Several machines are online: {0}. Pick one.")]
    PickOne(String),
    #[error("{0} has not said where its home folder is yet. Try again in a minute.")]
    NoHome(String),
    #[error(
        "Could not find {repo} on {host}. If it is private, add a token for {host} in Settings > Credentials."
    )]
    NotFound { repo: String, host: String },
    #[error(
        "Could not find {repo} on {host}, or your token there has no access to it. Check it in Settings > Credentials."
    )]
    NoAccess { repo: String, host: String },
    #[error("{machine} could not reach {host}.")]
    Unreachable { machine: String, host: String },
    #[error("{0} is out of disk space.")]
    NoSpace(String),
    #[error(
        "{folder} on {machine} already holds other files. Move them, or import onto another machine."
    )]
    Occupied { folder: String, machine: String },
    #[error("{folder} on {machine} already holds another repository ({remote}).")]
    OtherRepo {
        folder: String,
        machine: String,
        remote: String,
    },
    #[error(
        "Give the folder as an absolute path, such as /srv/code, or one in the home folder, such as ~/code."
    )]
    BadFolder,
    #[error("{folder} on {machine} is a file, not a folder.")]
    NotAFolder { folder: String, machine: String },
    #[error("{machine} cannot open {folder}.")]
    Unreadable { folder: String, machine: String },
    #[error("{machine} did not say what is in that folder.")]
    NoListing { machine: String },
    #[error("git is not installed on {0}.")]
    NoGit(String),
    #[error("Cloning {0} took more than 30 minutes and was stopped.")]
    TimedOut(String),
    #[error("The clone failed on {machine}: {detail}")]
    Failed { machine: String, detail: String },
    #[error("Could not reach {machine}: {detail}")]
    Link { machine: String, detail: String },
    #[error("{0} is not imported.")]
    NotImported(String),
    #[error("{0} shares no folders.")]
    NoSharedFolders(String),
    #[error("{0} is pinned to the machines it is on.")]
    Pinned(String),
    #[error("{0} has its sync paused.")]
    SyncPaused(String),
    #[error("No online machine can give {0}.")]
    NoCopyOnline(String),
    #[error("{machine} cannot sync it: {why}")]
    CannotSync { machine: String, why: String },
    #[error("A name is letters, digits, '-', '_' and '.', and does not start with '.' or '-'.")]
    BadName,
    #[error("Pick a machine.")]
    NoMachine,
    #[error("A local project goes on one machine.")]
    LocalOnOne,
    #[error("You already have a project called {0}.")]
    ProjectExists(String),
    #[error("{0} has no git host, so it stays on its machine.")]
    LocalOnly(String),
    #[error("Add a token for {0} in Settings > Credentials first.")]
    NoToken(String),
    #[error("{repo} already exists on {host}.")]
    NameTaken { repo: String, host: String },
    #[error(
        "Your token for {host} cannot create repositories in {owner}. Check its scopes in Settings > Credentials."
    )]
    CannotCreate { owner: String, host: String },
    #[error("{0}")]
    Forge(HostError),
    #[error("git init failed on {machine}: {detail}")]
    InitFailed { machine: String, detail: String },
    /// Beep's own failure, such as the database; not the user's to fix.
    #[error("Beep could not import the project.")]
    Internal,
}

impl From<anyhow::Error> for ImportError {
    fn from(e: anyhow::Error) -> Self {
        log::warn!("project import failed: {e:#}");
        Self::Internal
    }
}
