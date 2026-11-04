//! Importing a project: cloning a repository onto a user's machine through its node's shell, with
//! the user's token. The clone, the folders and the host calls are in `beep-projects`; this is
//! what the server adds: its machines, credentials and hosts, and the pages it lists.

mod checks;
mod context;
mod copy;
mod create;
mod host_repos;
mod remove;
mod service;
mod sync;
mod tool;

pub use checks::Checkouts;
pub(crate) use copy::copy_project;
pub(crate) use create::{create_project, project_forges};
pub use host_repos::HostRepoCache;
pub(crate) use host_repos::host_repos;
pub(crate) use remove::{machine_removed, remove_project};
pub(crate) use service::{
    browse, checkout_for_worktree, find_fork, found_projects, import, imported, preview,
    project_machines,
};
pub(crate) use tool::ImportProjectTool;
